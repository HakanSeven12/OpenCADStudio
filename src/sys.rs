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

/// If the main application window is minimized, restore it quietly in the background
/// without activating it or stealing user focus, so viewport captures succeed.
#[cfg(target_os = "windows")]
pub fn restore_window_if_minimized() -> bool {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
        SetWindowPos, ShowWindow, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SW_SHOWNOACTIVATE,
    };
    type BOOL = i32;

    struct Context {
        pid: u32,
        restored: bool,
    }

    unsafe {
        let current_pid = GetCurrentProcessId();
        let mut ctx = Context {
            pid: current_pid,
            restored: false,
        };

        unsafe extern "system" fn enum_wnd(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let ctx = &mut *(lparam as *mut Context);
            let mut wnd_pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut wnd_pid);
            if wnd_pid == ctx.pid {
                let mut class_buf = [0u16; 64];
                let len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), 64);
                let class_str = String::from_utf16_lossy(&class_buf[..len as usize]);

                // CRITICAL FIX: Only touch the main application window ("Window Class").
                // NEVER touch "Winit Thread Event Target" or hidden helper windows!
                if class_str.starts_with("Window Class") {
                    if IsIconic(hwnd) != 0 {
                        let fg = GetForegroundWindow();
                        // Restore in background without activation (never steal user focus)
                        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                        // Ensure it stays behind active user applications
                        if fg != hwnd && !fg.is_null() {
                            SetWindowPos(
                                hwnd,
                                HWND_BOTTOM,
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                            );
                        }
                        ctx.restored = true;
                    }
                }
            }
            1
        }
        EnumWindows(Some(enum_wnd), (&mut ctx as *mut Context) as LPARAM);
        ctx.restored
    }
}

#[cfg(not(target_os = "windows"))]
pub fn restore_window_if_minimized() -> bool {
    false
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

/// Web: is the page visible? `requestAnimationFrame` — and therefore the
/// frames subscription and any canvas readback — never runs while
/// `document.hidden`, so the control `capture` op refuses to wait for a frame
/// that could never come.
#[cfg(target_arch = "wasm32")]
pub fn page_visible() -> bool {
    !web_sys::window()
        .and_then(|window| window.document())
        .is_some_and(|document| document.hidden())
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

/// Extension the pasted-image registry name carries for a clipboard MIME
/// type. Unknown image types fall back to `png` — the flavor every OS
/// screenshot paste uses — so the reference still names a decodable format.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // tested everywhere, used on wasm
pub(crate) fn clipboard_image_extension(mime_type: &str) -> &'static str {
    match mime_type.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" | "image/x-ms-bmp" | "image/x-bmp" => "bmp",
        _ => "png",
    }
}

#[test]
fn clipboard_image_extensions_cover_the_paste_flavors() {
    assert_eq!(clipboard_image_extension("image/png"), "png");
    assert_eq!(clipboard_image_extension("IMAGE/JPEG"), "jpg");
    assert_eq!(clipboard_image_extension(" image/webp "), "webp");
    assert_eq!(clipboard_image_extension("image/gif"), "gif");
    assert_eq!(clipboard_image_extension("image/x-ms-bmp"), "bmp");
    // Anything else (a vendor flavor, a bare "image/*") still gets a name
    // the image registry can key and the `image` crate can decode.
    assert_eq!(clipboard_image_extension("image/avif"), "png");
    assert_eq!(clipboard_image_extension(""), "png");
}

/// Web: read an image from the system clipboard via the async Clipboard API
/// (`navigator.clipboard.read()`), used when a paste found no text. Returns
/// the registry name (`pasted-YYYYMMDD-HHMMSS-mmm[-N].<ext>`, unique against
/// the registry) plus the image bytes,
/// or `None` when the clipboard holds no image item or the read is denied.
/// Like `read_clipboard_text`, the Ctrl+V keypress that reaches it is a user
/// gesture, which is what makes the permission-gated read allowed at all.
#[cfg(target_arch = "wasm32")]
pub async fn read_clipboard_image() -> Option<(String, Vec<u8>)> {
    use wasm_bindgen::JsCast;

    let clipboard = web_sys::window()?.navigator().clipboard();
    let items = wasm_bindgen_futures::JsFuture::from(clipboard.read())
        .await
        .ok()?;
    let items = js_sys::Array::from(&items);
    for index in 0..items.length() {
        let Ok(item) = items.get(index).dyn_into::<web_sys::ClipboardItem>() else {
            continue;
        };
        // A clipboard item can offer several flavors; take the first image
        // one (browsers list at most one per item in practice). An item with
        // no image flavor just moves the search to the next one.
        let Some(mime_type) = item
            .types()
            .iter()
            .filter_map(|kind| kind.as_string())
            .find(|mime| mime.starts_with("image/"))
        else {
            continue;
        };
        let blob = wasm_bindgen_futures::JsFuture::from(item.get_type(&mime_type))
            .await
            .ok()?
            .dyn_into::<web_sys::Blob>()
            .ok()?;
        let buffer = wasm_bindgen_futures::JsFuture::from(blob.array_buffer())
            .await
            .ok()?;
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        if bytes.is_empty() {
            return None;
        }
        let name = pasted_image_name(&mime_type);
        // Register under the chosen name BEFORE returning: no other wasm
        // code can run between the uniqueness check and this insert (one
        // thread), so a second paste landing in the same millisecond sees
        // this name as taken and takes a `-2` suffix instead of overwriting
        // these bytes. The paste handler's own `register_source` call with
        // the same name is then an idempotent overwrite of identical bytes.
        crate::scene::model::image_model::register_source(
            &name,
            std::sync::Arc::new(bytes.clone()),
        );
        return Some((name, bytes));
    }
    None
}

/// Registry name for a pasted clipboard image:
/// `pasted-YYYYMMDD-HHMMSS-mmm.ext` stamped from the wall clock (local time,
/// the same clock a screenshot file name uses) at millisecond precision, with
/// the extension derived from the MIME type. Milliseconds alone don't
/// guarantee uniqueness (two pastes inside one ms), so the name is pushed
/// through [`unique_pasted_image_name`] against the live registry — a reused
/// key would overwrite the earlier paste's bytes (the registry is last-wins)
/// and make its entity display the newer pixels.
#[cfg(target_arch = "wasm32")]
fn pasted_image_name(mime_type: &str) -> String {
    let now = js_sys::Date::new_0();
    unique_pasted_image_name(
        &format!(
            "pasted-{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}",
            now.get_full_year(),
            now.get_month() + 1,
            now.get_date(),
            now.get_hours(),
            now.get_minutes(),
            now.get_seconds(),
            (now.get_time() as u64) % 1000,
        ),
        clipboard_image_extension(mime_type),
        crate::scene::model::image_model::has_source,
    )
}

/// Turn `base` + `ext` into a registry name not taken yet: the bare
/// `base.ext` first, then `-2`, `-3`, … inserted before the extension while
/// the candidate already names a registered source. The taken-check is
/// injectable so this — the part worth locking with tests — runs natively;
/// wasm passes [`crate::scene::model::image_model::has_source`].
pub(crate) fn unique_pasted_image_name(
    base: &str,
    ext: &str,
    taken: impl Fn(&str) -> bool,
) -> String {
    let first = format!("{base}.{ext}");
    if !taken(&first) {
        return first;
    }
    for suffix in 2u64.. {
        let candidate = format!("{base}-{suffix}.{ext}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    unreachable!("an unbounded suffix space always yields a free name")
}

#[test]
fn pasted_image_names_never_reuse_a_taken_registry_key() {
    let mut taken: Vec<String> = Vec::new();
    let claim = |taken: &mut Vec<String>| {
        let name = unique_pasted_image_name("pasted-20260915-101500-123", "png", |candidate| {
            taken.iter().any(|used| used == candidate)
        });
        taken.push(name.clone());
        name
    };
    // Fresh registry: the bare millisecond-stamped name is used as-is.
    assert_eq!(claim(&mut taken), "pasted-20260915-101500-123.png");
    // A second paste inside the same millisecond suffixes instead of
    // overwriting — each name below must differ from every earlier one.
    assert_eq!(claim(&mut taken), "pasted-20260915-101500-123-2.png");
    assert_eq!(claim(&mut taken), "pasted-20260915-101500-123-3.png");
    // The extension is part of the key: a jpg landing in the same
    // millisecond does not collide with the pngs above.
    assert_eq!(
        unique_pasted_image_name("pasted-20260915-101500-123", "jpg", |candidate| {
            taken.iter().any(|used| used == candidate)
        }),
        "pasted-20260915-101500-123.jpg"
    );
    // Freed suffixes are reused in order: with `-2` unregistered again, the
    // next paste takes `-2` back before walking further.
    taken.remove(1);
    assert_eq!(claim(&mut taken), "pasted-20260915-101500-123-2.png");
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

// ── FILEDIA and file pickers that keep their file type ───────────────────────

static FILE_DIA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// FILEDIA (a profile setting): commands that have a command-line form show
/// their file dialogs only while it is on.
pub fn file_dia() -> bool {
    FILE_DIA.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_file_dia(on: bool) {
    FILE_DIA.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The file type ATTACH's picker was last left on (1-based; 7 = Drawing).
static ATTACH_FILTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(7);

pub fn attach_filter() -> u32 {
    ATTACH_FILTER.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_attach_filter(index: u32) {
    ATTACH_FILTER.store(index, std::sync::atomic::Ordering::Relaxed);
}

/// A file type of [`pick_file_with_filter`]: its label and extensions
/// (`"*"` for every file).
pub type FileFilter = (String, &'static [&'static str]);

/// Open-file dialog that starts on file type `index` (1-based) and returns the
/// picked path with the file type the dialog was left on.
#[cfg(target_os = "windows")]
pub async fn pick_file_with_filter(
    title: String,
    filters: Vec<FileFilter>,
    index: u32,
) -> Option<(std::path::PathBuf, u32)> {
    // The dialog blocks, so it runs on its own thread like the async picker.
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(open_file_dialog_blocking(&title, &filters, index));
    });
    rx.await.ok().flatten()
}

#[cfg(target_os = "windows")]
fn open_file_dialog_blocking(
    title: &str,
    filters: &[FileFilter],
    index: u32,
) -> Option<(std::path::PathBuf, u32)> {
    use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let mut filter = Vec::new();
    for (label, extensions) in filters {
        let pattern = extensions
            .iter()
            .map(|e| format!("*.{e}"))
            .collect::<Vec<_>>()
            .join(";");
        filter.extend(wide(label));
        filter.extend(wide(&pattern));
    }
    filter.push(0);
    let title = wide(title);
    let dir = crate::config::last_dialog_dir().map(|d| wide(&d.to_string_lossy()));
    let mut file = vec![0u16; 32768];
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter.as_ptr(),
        nFilterIndex: index,
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        lpstrTitle: title.as_ptr(),
        lpstrInitialDir: dir.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
        Flags: OFN_EXPLORER
            | OFN_FILEMUSTEXIST
            | OFN_PATHMUSTEXIST
            | OFN_NOCHANGEDIR
            | OFN_HIDEREADONLY,
        ..Default::default()
    };
    // SAFETY: every pointer in `ofn` points into buffers that outlive the call.
    let picked = unsafe {
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        GetOpenFileNameW(&mut ofn)
    };
    if picked == 0 {
        return None;
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    let path = std::path::PathBuf::from(String::from_utf16_lossy(&file[..len]));
    crate::config::remember_dialog_dir(&path);
    Some((path, ofn.nFilterIndex))
}

/// Elsewhere the picker cannot start on a file type; the index passes through.
#[cfg(not(target_os = "windows"))]
pub async fn pick_file_with_filter(
    title: String,
    filters: Vec<FileFilter>,
    index: u32,
) -> Option<(std::path::PathBuf, u32)> {
    let mut dialog = file_dialog().set_title(title);
    for (label, extensions) in &filters {
        dialog = dialog.add_filter(label, extensions);
    }
    let handle = dialog.pick_file().await?;
    Some((handle_path(&handle), index))
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
    let array = js_sys::Uint8Array::from(bytes);
    // A host page that embeds the app takes the file instead of a download.
    if let Some(hook) = save_hook() {
        let _ = hook.call2(&window, &wasm_bindgen::JsValue::from_str(name), &array);
        return;
    }
    let Some(document) = window.document() else {
        return;
    };
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

/// The `window.openCadStudioSaveFile(name, bytes)` function an embedding page
/// registers to receive saved and exported files, if any.
#[cfg(target_arch = "wasm32")]
pub fn save_hook() -> Option<js_sys::Function> {
    use wasm_bindgen::JsCast;
    let window = web_sys::window()?;
    js_sys::Reflect::get(window.as_ref(), &wasm_bindgen::JsValue::from_str("openCadStudioSaveFile"))
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()
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

/// Desktop crash log.
///
/// A release build hides the console (`windows_subsystem = "windows"`) and is
/// stripped, so a panic used to end the process with nothing on screen, nothing
/// on stderr and nothing on disk: the application simply vanished. That is what
/// every "it just closes" report has had to work from (#635, #845), and why a
/// crash like the TRIM one (#830) could be reported for months with no detail
/// beyond the steps.
///
/// Every panic now leaves one small file behind naming what failed and where.
/// The hook chains onto whatever was installed before it, so a debug build
/// still prints to stderr as well.
#[cfg(not(target_arch = "wasm32"))]
pub mod crash_log {
    use std::path::PathBuf;

    /// Newest reports to keep. A crash loop must not fill the user's profile,
    /// and the first report of a loop is the interesting one, so keep both
    /// ends by pruning only what is older than this.
    const KEEP: usize = 20;

    /// Where reports go, beside the recovery logs.
    pub fn directory() -> Option<PathBuf> {
        crate::config::config_dir().map(|path| path.join("crash_logs"))
    }

    /// The report a given process left behind, if it left one.
    ///
    /// Reports are named for the process that wrote them, so a caller holding
    /// a process id from somewhere else — the GPU crash sentinel does — can
    /// ask what killed that exact run instead of guessing from the newest
    /// file, which on a busy machine may belong to a different one.
    ///
    /// Process ids are reused, so only a report written at or after
    /// `not_before` (Unix seconds — when that run started) counts: an old
    /// report from an unrelated run that once had the same id does not.
    pub fn report_for_pid(pid: u32, not_before: u64) -> Option<String> {
        let directory = directory()?;
        let suffix = format!("-{pid}.log");
        let entry = std::fs::read_dir(directory).ok()?.flatten().find(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_prefix("crash-")
                .and_then(|rest| rest.strip_suffix(suffix.as_str()))
                .and_then(|when| when.parse::<u64>().ok())
                .is_some_and(|when| when >= not_before)
        })?;
        std::fs::read_to_string(entry.path()).ok()
    }

    /// Whether a report describes the graphics device giving up.
    ///
    /// Deliberately narrow. Anything that reaches wgpu — an exhausted device,
    /// a lost one, a buffer that was never created — is the backend's
    /// business; a panic in the drawing code that happens to run on a frame
    /// is not, and must not cost the user a working backend.
    pub fn is_device_failure(report: &str) -> bool {
        let mut panic_line = "";
        let mut at_line = "";
        for line in report.lines() {
            if let Some(rest) = line.strip_prefix("panic: ") {
                panic_line = rest;
            } else if let Some(rest) = line.strip_prefix("at: ") {
                at_line = rest;
            }
        }
        // The message wgpu panics with names the call, and the location names
        // wgpu itself; either alone is enough, since a stripped build can
        // leave the message terse. Plain words like "buffer" or "surface"
        // are not: drawing code and the modelling kernel panic with those
        // too, and that is no reason to give up a working backend.
        let haystack = format!("{panic_line} {at_line}").to_ascii_lowercase();
        ["wgpu", "out of memory", "device lost", "validation error"]
            .iter()
            .any(|needle| haystack.contains(needle))
    }

    /// The backend a report says was live, as written by [`report_from`].
    pub fn backend_in_report(report: &str) -> Option<String> {
        report
            .lines()
            .find_map(|line| line.strip_prefix("gpu backend: "))
            .map(str::trim)
            .filter(|backend| !backend.is_empty() && *backend != "(default)")
            .map(str::to_string)
    }

    /// Chain a report writer onto the current panic hook.
    ///
    /// Call once, early: a panic before this runs is still silent.
    pub fn install() {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // A panic while reporting a panic must not replace the original
            // with a recursion, so failures here are dropped on purpose.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| write(info)));
            previous(info);
        }));
    }

    /// The report body. Separated from the IO so a test can read it.
    pub fn report(info: &std::panic::PanicHookInfo<'_>, when: u64) -> String {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_string());
        let where_ = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "(unknown location)".to_string());
        report_from(&payload, &where_, when)
    }

    /// The report body, with the two pieces a panic carries already pulled
    /// out. Separate so a test can exercise the layout without installing a
    /// panic hook: the hook is process-global, and a test that asserts inside
    /// one fires on every other test's panic and aborts the whole binary.
    pub fn report_from(payload: &str, where_: &str, when: u64) -> String {
        // `WGPU_BACKEND` is what the backend resolver settled on, so the
        // report says which graphics path was live without reaching into the
        // renderer from a panic handler.
        let backend = std::env::var("WGPU_BACKEND").unwrap_or_else(|_| "(default)".to_string());
        // `force_capture`, not `capture`: a report whose backtrace says
        // "disabled backtrace" because the user never set RUST_BACKTRACE is
        // exactly the report that helps nobody. Release builds are stripped,
        // so some frames are addresses, but the frame count and the module
        // boundaries still place the fault.
        let backtrace = std::backtrace::Backtrace::force_capture();
        [
            format!("Open CAD Studio {} crash report", env!("OCS_APP_VERSION")),
            format!("when: {when} (unix seconds)"),
            format!("os: {} {}", std::env::consts::OS, std::env::consts::ARCH),
            format!("gpu backend: {backend}"),
            format!(
                "thread: {}",
                std::thread::current().name().unwrap_or("unnamed")
            ),
            format!("panic: {payload}"),
            format!("at: {where_}"),
            String::new(),
            "backtrace:".to_string(),
            backtrace.to_string(),
            String::new(),
        ]
        .join("\n")
    }

    fn write(info: &std::panic::PanicHookInfo<'_>) {
        let Some(directory) = directory() else {
            return;
        };
        if std::fs::create_dir_all(&directory).is_err() {
            return;
        }
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let name = format!("crash-{when}-{}.log", std::process::id());
        let path = directory.join(name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        if let Ok(mut file) = options.open(&path) {
            use std::io::Write;
            let _ = file.write_all(report(info, when).as_bytes());
        }
        prune(&directory);
    }

    /// Keep the newest [`KEEP`] reports.
    fn prune(directory: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
            .filter_map(|e| {
                let modified = e.metadata().ok()?.modified().ok()?;
                Some((modified, e.path()))
            })
            .collect();
        if files.len() <= KEEP {
            return;
        }
        files.sort_by_key(|(when, _)| *when);
        for (_, path) in &files[..files.len() - KEEP] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[cfg(test)]
    mod tests {
        /// The report has to carry the two things a maintainer cannot guess:
        /// what the message was and where it came from. It also has to be
        /// readable — an earlier version built it with `\` continuations and
        /// pasted the source indentation into every line.
        #[test]
        fn a_report_names_the_message_and_the_line() {
            let text = super::report_from("a deliberate test panic", "src/sys.rs:1:2", 42);
            assert!(text.starts_with("Open CAD Studio "), "{text}");
            assert!(text.contains("panic: a deliberate test panic"), "{text}");
            assert!(text.contains("at: src/sys.rs:1:2"), "{text}");
            assert!(text.contains("when: 42 (unix seconds)"), "{text}");
            let (header, trace) = text
                .split_once("\nbacktrace:\n")
                .expect("the report carries a backtrace section");
            assert!(!trace.trim().is_empty(), "the backtrace must not be empty");
            // Header only: a backtrace indents its own continuation lines,
            // and that is how a backtrace is meant to read.
            assert!(
                header.lines().all(|line| !line.starts_with(' ')),
                "header lines must not be indented:\n{header}"
            );
        }

        /// Only the device's own failures may cost a backend. A panic in
        /// drawing code that merely happened to run on a frame must not.
        #[test]
        fn a_device_failure_is_told_apart_from_an_ordinary_panic() {
            let device = super::report_from(
                "Error in Buffer::get_mapped_range: Validation Error",
                "wgpu-29.0.4/src/backend/wgpu_core.rs:2253:18",
                1,
            );
            assert!(super::is_device_failure(&device), "{device}");

            let oom = super::report_from("Out of Memory", "src/scene/pipeline/mod.rs:10:1", 1);
            assert!(super::is_device_failure(&oom), "{oom}");

            let ordinary = super::report_from(
                "index out of bounds: the len is 3 but the index is 7",
                "src/app/commands/draw.rs:120:5",
                1,
            );
            assert!(
                !super::is_device_failure(&ordinary),
                "an indexing bug is not the graphics device's fault:
{ordinary}"
            );
        }

        /// The backend line is what the sentinel joins on, so it has to come
        /// back out — and "(default)" names nothing to blame.
        #[test]
        fn the_backend_is_read_back_out_of_a_report() {
            // SAFETY: single-threaded test mutating a process-local variable.
            unsafe { std::env::set_var("WGPU_BACKEND", "dx12") };
            let named = super::report_from("boom", "x.rs:1:1", 0);
            assert_eq!(super::backend_in_report(&named).as_deref(), Some("dx12"));

            unsafe { std::env::remove_var("WGPU_BACKEND") };
            let unnamed = super::report_from("boom", "x.rs:1:1", 0);
            assert_eq!(super::backend_in_report(&unnamed), None);
        }

        /// A panic with no message still has to produce a filed report rather
        /// than nothing at all.
        #[test]
        fn a_report_survives_a_panic_with_nothing_to_say() {
            let text = super::report_from("(no message)", "(unknown location)", 0);
            assert!(text.contains("panic: (no message)"), "{text}");
            assert!(text.contains("at: (unknown location)"), "{text}");
        }
    }
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
