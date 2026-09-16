// Small platform shims for things the desktop build does natively but the web
// (wasm) build must handle differently or skip.

/// Drawing hyperlinks may open web pages, never local files or custom handlers.
pub(crate) fn web_hyperlink(value: &str) -> Option<String> {
    if value.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(value.trim()).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host_str().is_some()).then(|| url.into())
}

#[test]
fn drawing_hyperlinks_only_open_web_pages() {
    for value in ["https://example.com/path", " HTTP://example.com "] {
        assert!(web_hyperlink(value).is_some(), "{value}");
    }
    for value in [
        "",
        "https://",
        "javascript:alert(1)",
        "data:text/html,example",
        "file:///tmp/program.desktop",
        "/tmp/program.desktop",
        "custom:run",
        "mailto:user@example.com",
        "https://example.com/\npath",
    ] {
        assert!(web_hyperlink(value).is_none(), "{value}");
    }
}

/// Open a URL in the user's browser.
///
/// Wayland requires an xdg-activation token from the source window before it
/// will let an existing browser window take focus. Linux obtains that token
/// from the OCS surface and passes it to xdg-open.
#[cfg(target_os = "linux")]
pub fn open_url<Message: Send + 'static>(
    url: &str,
    parent: Option<iced::window::Id>,
) -> iced::Task<Message> {
    let url = url.to_string();
    match parent {
        Some(parent) => iced::window::run(parent, move |window| {
            open_url_linux(&url, linux_activation_token(window));
        })
        .discard(),
        None => {
            let _ = open::that_detached(url);
            iced::Task::none()
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_activation_token(window: &dyn iced::window::Window) -> Option<String> {
    use iced::window::raw_window_handle::{RawDisplayHandle, RawWindowHandle};

    let RawWindowHandle::Wayland(window_handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    let RawDisplayHandle::Wayland(display_handle) = window.display_handle().ok()?.as_raw() else {
        return None;
    };

    // Keep the iced window borrowed while ashpd uses its raw Wayland surface.
    // The returned token is an owned string and is immediately handed to the
    // child process.
    unsafe {
        iced::futures::executor::block_on(ashpd::ActivationToken::from_wayland_raw(
            None,
            window_handle.surface.as_ptr(),
            display_handle.display.as_ptr(),
        ))
    }
    .map(String::from)
}

#[cfg(target_os = "linux")]
fn open_url_linux(url: &str, activation_token: Option<String>) {
    let Some(token) = activation_token else {
        let _ = open::that_detached(url);
        return;
    };

    let mut command = std::process::Command::new("xdg-open");
    command
        .arg(url)
        .env("XDG_ACTIVATION_TOKEN", &token)
        .env("DESKTOP_STARTUP_ID", token);

    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    } else {
        let _ = open::that_detached(url);
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "linux")))]
pub fn open_url<Message>(url: &str, _parent: Option<iced::window::Id>) -> iced::Task<Message> {
    let _ = open::that_detached(url);
    iced::Task::none()
}

/// Web opens the tab synchronously so the browser still sees the click as a
/// user gesture and does not block it as a pop-up.
#[cfg(target_arch = "wasm32")]
pub fn open_url<Message>(url: &str, _parent: Option<iced::window::Id>) -> iced::Task<Message> {
    if let Some(window) = web_sys::window() {
        let _ = window.open_with_url_and_target_and_features(url, "_blank", "noopener,noreferrer");
    }
    iced::Task::none()
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static BEFORE_UNLOAD_WARNING: std::cell::RefCell<
        Option<wasm_bindgen::closure::Closure<dyn FnMut(web_sys::BeforeUnloadEvent)>>
    > = const { std::cell::RefCell::new(None) };
}

/// Enable the browser's standard leave-page confirmation while drawings have
/// unsaved changes. Browsers control the dialog text; preventing the event and
/// setting `returnValue` are the portable signal that a warning is required.
///
/// The callback is installed only while needed so clean sessions remain
/// eligible for the browser back/forward cache.
#[cfg(target_arch = "wasm32")]
pub fn set_unsaved_changes_warning(active: bool) {
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else {
        return;
    };
    BEFORE_UNLOAD_WARNING.with(|warning| {
        let mut warning = warning.borrow_mut();
        if active == warning.is_some() {
            return;
        }
        if active {
            let callback =
                wasm_bindgen::closure::Closure::new(move |event: web_sys::BeforeUnloadEvent| {
                    event.prevent_default();
                    event.set_return_value("");
                });
            window.set_onbeforeunload(Some(callback.as_ref().unchecked_ref()));
            *warning = Some(callback);
        } else {
            window.set_onbeforeunload(None);
            warning.take();
        }
    });
}

/// Reveal a saved drawing in the native platform's file manager.
#[cfg(not(target_arch = "wasm32"))]
pub fn reveal_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer.exe")
        .arg("/select,")
        .arg(path)
        .status();

    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .status();

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let folder = path
            .parent()
            .ok_or_else(|| format!("Path has no parent folder: {}", path.display()))?;
        return open::that(folder).map_err(|e| e.to_string());
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("File manager exited with {status}")),
        Err(error) => Err(error.to_string()),
    }
}

/// Copy the rendered web canvas during the frame callback, before the browser
/// clears its drawing buffer. Canvas readback avoids Iced's synchronous GPU map.
#[cfg(target_arch = "wasm32")]
pub fn capture_canvas() -> Option<iced::window::Screenshot> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window()?;
    let document = window.document()?;
    let source = document
        .query_selector("canvas")
        .ok()??
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()?;
    let (width, height) = (source.width(), source.height());
    if width == 0 || height == 0 {
        return None;
    }
    let copy = document
        .create_element("canvas")
        .ok()?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()?;
    copy.set_width(width);
    copy.set_height(height);
    let context = copy
        .get_context("2d")
        .ok()??
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .ok()?;
    context
        .draw_image_with_html_canvas_element(&source, 0.0, 0.0)
        .ok()?;
    let pixels = context
        .get_image_data(0.0, 0.0, width as f64, height as f64)
        .ok()?;
    Some(iced::window::Screenshot::new(
        pixels.data().0,
        iced::Size::new(width, height),
        window.device_pixel_ratio() as f32,
    ))
}

/// Web: read text from the system clipboard via the async Clipboard API.
/// iced's own `clipboard::read` is a no-op on the web (the browser clipboard is
/// async + permission-gated), so the editor paste paths use this instead. The
/// Ctrl+V keypress that drives it is a user gesture, so the read is permitted.
/// Returns `None` when denied, empty, or unsupported.
#[cfg(target_arch = "wasm32")]
pub async fn read_clipboard_text() -> Option<String> {
    let clipboard = web_sys::window()?.navigator().clipboard();
    let value = wasm_bindgen_futures::JsFuture::from(clipboard.read_text())
        .await
        .ok()?;
    value.as_string()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(module = "/web/clipboard.js")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = copyHistory)]
    pub fn copy_history_text(
        text: &str,
        fallback_label: &str,
        close_label: &str,
    ) -> js_sys::Promise;
}

/// Web: write text to the system clipboard (fire-and-forget). Backs Ctrl+C in
/// plain text_input fields, whose iced-internal copy is a no-op on the web
/// (#346). The triggering keypress is a user gesture, so the write is allowed.
#[cfg(target_arch = "wasm32")]
pub fn write_clipboard_text(text: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().clipboard().write_text(text);
    }
}

/// Web: replay `text` into the focused iced widget as synthetic KeyboardEvents
/// dispatched on the winit canvas. This is the only route INTO a focused
/// text_input on the web — iced's clipboard read is a no-op there and it
/// exposes no insert-text operation (#346). Control characters are dropped
/// and the length capped so a runaway clipboard can't wedge the event loop.
#[cfg(target_arch = "wasm32")]
pub fn synthesize_typing(text: &str) {
    let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector("canvas").ok().flatten())
    else {
        return;
    };
    for ch in text.chars().filter(|c| !c.is_control()).take(1024) {
        let init = web_sys::KeyboardEventInit::new();
        init.set_key(&ch.to_string());
        init.set_bubbles(true);
        init.set_cancelable(true);
        for kind in ["keydown", "keyup"] {
            if let Ok(ev) = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict(kind, &init) {
                let _ = canvas.dispatch_event(&ev);
            }
        }
    }
}

/// Turn an `rfd` file handle into a `PathBuf` the rest of the app keys on.
///
/// Desktop returns the real filesystem path. The browser has no path, so we
/// synthesize one from the file name — enough for the app to compile and track
/// the document name; actual byte I/O on the web reads the handle directly
/// (a follow-up).
#[cfg(not(target_arch = "wasm32"))]
pub fn handle_path(h: &rfd::FileHandle) -> std::path::PathBuf {
    let p = h.path().to_path_buf();
    // Every dialog result funnels through here — remember its folder so the
    // next dialog opens where the user left off.
    crate::config::remember_dialog_dir(&p);
    p
}

#[cfg(target_arch = "wasm32")]
pub fn handle_path(h: &rfd::FileHandle) -> std::path::PathBuf {
    std::path::PathBuf::from(h.file_name())
}

/// New async file dialog seeded with the last directory a dialog was used in.
/// All pickers should start from this instead of `rfd::AsyncFileDialog::new()`.
#[cfg(not(target_arch = "wasm32"))]
pub fn file_dialog() -> rfd::AsyncFileDialog {
    let dlg = rfd::AsyncFileDialog::new();
    match crate::config::last_dialog_dir() {
        Some(dir) => dlg.set_directory(dir),
        None => dlg,
    }
}

/// Blocking native file dialog seeded with the last-used directory.
///
/// Use this through `iced::window::run` when the picker needs the main
/// window's raw handle. Portal-based desktops can otherwise reject or lose a
/// parentless save request without ever showing a dialog (#537).
#[cfg(not(target_arch = "wasm32"))]
pub fn blocking_file_dialog() -> rfd::FileDialog {
    let dlg = rfd::FileDialog::new();
    match crate::config::last_dialog_dir() {
        Some(dir) => dlg.set_directory(dir),
        None => dlg,
    }
}

/// Web: no filesystem paths, nothing to remember.
#[cfg(target_arch = "wasm32")]
pub fn file_dialog() -> rfd::AsyncFileDialog {
    rfd::AsyncFileDialog::new()
}

/// Trigger a browser download of `bytes` as `name`. Builds a Blob, points a
/// hidden `<a download>` at it and clicks it programmatically — because this
/// runs inside the Save button's click (a user gesture), the file downloads
/// immediately with no extra "click to download" link. Web only.
#[cfg(target_arch = "wasm32")]
pub fn download_bytes(name: &str, bytes: &[u8]) {
    use wasm_bindgen::JsCast;
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence(&parts) else {
        return;
    };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else {
        return;
    };
    if let Ok(el) = document.create_element("a") {
        let a: web_sys::HtmlAnchorElement = el.unchecked_into();
        a.set_href(&url);
        a.set_download(name);
        a.click();
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}

/// Show a plot preview as an in-page overlay. The plotted PDF arrives already
/// rasterised to PNG pages: inline `<img>` elements render in every browser,
/// with no popup permission and no PDF viewer plugin — both of which embedded
/// browser hosts routinely withhold. A Download button still hands out the
/// vector PDF, Close drops the overlay. Web only.
#[cfg(target_arch = "wasm32")]
pub fn show_pdf_preview(name: &str, pdf_bytes: &[u8], page_pngs: &[Vec<u8>], total_pages: usize) {
    use wasm_bindgen::JsCast;
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    // One preview at a time: a replotted preview replaces its predecessor,
    // revokes its blob URLs and DROPS its Download closure (freeing the full
    // PDF copy the closure captured — a `forget()` would leak one per
    // preview), so repeated previews cannot stack overlays or leak the
    // images underneath.
    const OVERLAY_ID: &str = "ocs-plot-preview";
    if let Some(previous) = document.get_element_by_id(OVERLAY_ID) {
        previous.remove();
    }
    for url in preview_urls()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .drain(..)
    {
        let _ = web_sys::Url::revoke_object_url(&url);
    }
    // Take the previous closure out so it destructs BEFORE the new preview
    // stores its own: removing the overlay element already released the JS
    // handler reference, so this drop is the last one and actually frees
    // the captured bytes.
    drop(PREVIEW_DOWNLOAD_CLOSURE.with(|slot| slot.borrow_mut().take()));
    if page_pngs.is_empty() {
        // Rasterisation produced nothing: deliver the PDF itself rather than
        // show an empty frame.
        download_bytes(name, pdf_bytes);
        return;
    }
    let Ok(el) = document.create_element("div") else {
        download_bytes(name, pdf_bytes);
        return;
    };
    let backdrop: web_sys::HtmlElement = el.unchecked_into();
    backdrop.set_id(OVERLAY_ID);
    backdrop
        .set_attribute(
            "style",
            "position:fixed;inset:0;background:rgba(0,0,0,.88);z-index:30000;overflow:auto;\
             display:flex;flex-direction:column;align-items:center;gap:14px;padding:24px;\
             font-family:system-ui,sans-serif;color:#ddd",
        )
        .ok();

    let mut urls = Vec::new();
    for png in page_pngs {
        let array = js_sys::Uint8Array::from(png.as_slice());
        let parts = js_sys::Array::new();
        parts.push(&array.buffer());
        let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence(&parts) else {
            continue;
        };
        let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else {
            continue;
        };
        if let Ok(el) = document.create_element("img") {
            let img: web_sys::HtmlImageElement = el.unchecked_into();
            img.set_src(&url);
            img.set_attribute(
                "style",
                "max-width:min(92vw,1000px);height:auto;background:#fff;\
                 box-shadow:0 2px 14px rgba(0,0,0,.6)",
            )
            .ok();
            backdrop.append_child(&img).ok();
            urls.push(url);
        }
    }
    if urls.is_empty() {
        download_bytes(name, pdf_bytes);
        return;
    }
    preview_urls()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .extend(urls.iter().cloned());

    // Sticky action bar so Download/Close stay reachable while pages scroll.
    let bar: web_sys::HtmlElement = document
        .create_element("div")
        .ok()
        .and_then(|el| {
            let bar: web_sys::HtmlElement = el.unchecked_into();
            bar.set_attribute(
                "style",
                "position:sticky;top:0;display:flex;gap:12px;align-items:center;\
                 background:rgba(0,0,0,.88);padding:4px 8px",
            )
            .ok()?;
            Some(bar)
        })
        .unwrap_or_else(|| backdrop.clone());
    if let Ok(el) = document.create_element("span") {
        let label: web_sys::HtmlElement = el.unchecked_into();
        label.set_inner_text(&if total_pages > urls.len() {
            format!(
                "Plot preview — first {} of {} pages",
                urls.len(),
                total_pages
            )
        } else {
            format!(
                "Plot preview — {} page{}",
                urls.len(),
                if urls.len() == 1 { "" } else { "s" }
            )
        });
        bar.append_child(&label).ok();
    }
    let button = |text: &str| -> Option<web_sys::HtmlElement> {
        let el = document.create_element("button").ok()?;
        let btn: web_sys::HtmlElement = el.unchecked_into();
        btn.set_inner_text(text);
        btn.set_attribute(
            "style",
            "padding:8px 18px;border:0;border-radius:4px;cursor:pointer;\
             background:#3399e6;color:#fff;font-size:14px",
        )
        .ok()?;
        Some(btn)
    };
    if let Some(dl) = button("Download PDF") {
        let name = name.to_string();
        let bytes = pdf_bytes.to_vec();
        // A plain FnMut stays usable for every click; the one-shot variant
        // throws on the second click, and users click Download twice. The
        // closure is HELD in a registry (not `forget()`ed): replacing or
        // closing the overlay drops it and frees the captured PDF copy,
        // while the cloned JS function keeps the live handler working.
        let closure: wasm_bindgen::closure::Closure<dyn FnMut()> =
            wasm_bindgen::closure::Closure::new(move || {
                download_bytes(&name, &bytes);
            });
        let click: js_sys::Function = closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
        PREVIEW_DOWNLOAD_CLOSURE.with(|slot| *slot.borrow_mut() = Some(closure));
        dl.set_onclick(Some(&click));
        bar.append_child(&dl).ok();
    }
    if let Some(close) = button("Close") {
        let backdrop = backdrop.clone();
        let click = wasm_bindgen::closure::Closure::once_into_js(move || {
            for url in preview_urls()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .drain(..)
            {
                let _ = web_sys::Url::revoke_object_url(&url);
            }
            backdrop.remove();
        });
        let click: js_sys::Function = click.unchecked_into();
        close.set_onclick(Some(&click));
        bar.append_child(&close).ok();
    }
    if let Some(first) = backdrop.first_child() {
        let _ = backdrop.insert_before(&bar, Some(&first));
    }
    if let Some(body) = document.body() {
        body.append_child(&backdrop).ok();
    }
}

/// Blob URLs owned by the live preview overlay, revoked when it is closed or
/// replaced. They live outside the wasm heap and are not GC'd on their own.
#[cfg(target_arch = "wasm32")]
fn preview_urls() -> &'static std::sync::Mutex<Vec<String>> {
    static URLS: std::sync::OnceLock<std::sync::Mutex<Vec<String>>> = std::sync::OnceLock::new();
    URLS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

// The live preview overlay's Download-button closure. The closure captures
// a full copy of the PDF bytes, so `forget()`-ing it would leak one PDF per
// preview; holding it here lets the next `show_pdf_preview` drop and free
// the previous one (the overlay element's removal already released the JS
// handler reference). A thread-local `RefCell` rather than a static
// `Mutex` like `preview_urls`: `Closure` is not `Send`, and the overlay
// only ever lives on the one UI thread.
#[cfg(target_arch = "wasm32")]
thread_local! {
    static PREVIEW_DOWNLOAD_CLOSURE:
        std::cell::RefCell<Option<wasm_bindgen::closure::Closure<dyn FnMut()>>> =
        std::cell::RefCell::new(None);
}

/// Short platform string for bug reports: OS + architecture on the desktop,
/// the browser user-agent on the web.
#[cfg(not(target_arch = "wasm32"))]
pub fn platform_info() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(target_arch = "wasm32")]
pub fn platform_info() -> String {
    web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .map(|ua| format!("Web — {ua}"))
        .unwrap_or_else(|| "Web (wasm)".to_string())
}

/// Percent-encode `s` for use in a URL query value (e.g. a GitHub issue
/// `?body=`). Encodes everything outside the unreserved set.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Web renderer-error surface (#414): wgpu / naga report pipeline and shader
/// failures through the `log` facade and then leave the canvas empty — with no
/// logger installed the message is lost, so a broken GPU path looks like "the
/// app draws nothing" with a clean console. Mirror every Error-level record to
/// the browser console AND into a fixed DOM banner whose text is selectable
/// (the canvas UI is not), with a one-click Copy button, so a failing user can
/// paste the exact error into a bug report. Panics land in the same banner via
/// a chained hook.
#[cfg(target_arch = "wasm32")]
pub mod web_diag {
    use std::sync::Mutex;

    /// Cap on banner entries so a per-frame error can't grow the DOM forever.
    const MAX_LINES: u32 = 12;

    /// Last banner message + repeat count, for collapsing a hot error loop
    /// into one line with an `(xN)` suffix.
    static LAST: Mutex<(String, u32)> = Mutex::new((String::new(), 0));

    struct BannerLogger;

    impl log::Log for BannerLogger {
        fn enabled(&self, meta: &log::Metadata) -> bool {
            meta.level() <= log::Level::Error
        }
        fn log(&self, record: &log::Record) {
            if record.level() > log::Level::Error {
                return;
            }
            let msg = format!("[{}] {}", record.target(), record.args());
            web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&msg));
            show_banner(&msg);
        }
        fn flush(&self) {}
    }

    /// Install the logger + panic mirror. Call once at web startup, AFTER
    /// `console_error_panic_hook::set_once` so the chained hook keeps the
    /// console stack trace.
    pub fn init() {
        let console_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            show_banner(&info.to_string());
            console_hook(info);
        }));
        if log::set_boxed_logger(Box::new(BannerLogger)).is_ok() {
            log::set_max_level(log::LevelFilter::Error);
        }
    }

    /// Append `msg` to the on-page banner, creating the overlay on first use.
    fn show_banner(msg: &str) {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let pre = match doc.get_element_by_id("ocs-err-text") {
            Some(pre) => pre,
            None => {
                let Some(body) = doc.body() else { return };
                let Ok(overlay) = doc.create_element("div") else {
                    return;
                };
                overlay.set_id("ocs-err");
                let _ = overlay.set_attribute(
                    "style",
                    "position:fixed;top:0;left:0;right:0;z-index:2147483647;\
                     background:#5c1a1a;color:#ffdddd;font:12px monospace;\
                     padding:8px 12px;max-height:40vh;overflow:auto;\
                     user-select:text;cursor:text;",
                );
                overlay.set_inner_html(
                    "<div><b id=\"ocs-err-title\"></b> \
                     <button id=\"ocs-err-copy\" style=\"margin-left:8px\" onclick=\"navigator.clipboard.writeText(\
                     document.getElementById('ocs-err-text').innerText)\"></button> \
                     <button id=\"ocs-err-dismiss\" onclick=\"document.getElementById('ocs-err').remove()\"></button></div>\
                     <pre id=\"ocs-err-text\" style=\"margin:6px 0 0;\
                     white-space:pre-wrap;user-select:text;\"></pre>",
                );
                let _ = body.append_child(&overlay);
                for (id, label) in [
                    (
                        "ocs-err-title",
                        crate::t!("OpenCADStudio renderer error — copy this into a bug report:"),
                    ),
                    ("ocs-err-copy", crate::t!("Copy")),
                    ("ocs-err-dismiss", crate::t!("Dismiss")),
                ] {
                    if let Some(element) = doc.get_element_by_id(id) {
                        element.set_text_content(Some(label.as_ref()));
                    }
                }
                match doc.get_element_by_id("ocs-err-text") {
                    Some(pre) => pre,
                    None => return,
                }
            }
        };
        // Collapse repeats: an error thrown every frame becomes one line with
        // a running (xN) counter instead of MAX_LINES copies of itself.
        let mut last = match LAST.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if last.0 == msg {
            last.1 += 1;
            if let Some(line) = pre.last_element_child() {
                line.set_text_content(Some(&format!("{msg} (x{})", last.1)));
            }
            return;
        }
        *last = (msg.to_string(), 1);
        if pre.child_element_count() >= MAX_LINES {
            if let Some(first) = pre.first_element_child() {
                first.remove();
            }
        }
        if let Ok(line) = doc.create_element("div") {
            line.set_text_content(Some(msg));
            let _ = pre.append_child(&line);
        }
    }
}
