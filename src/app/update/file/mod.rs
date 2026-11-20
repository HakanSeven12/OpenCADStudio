//! `file` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{parse_coord, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis, CoordKind};
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::io::plot_style::PlotStyleTableExt as _;
use crate::modules::ModuleEvent;
use crate::scene::model::object::GripApply;
use crate::scene::pick::grip::{find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit};
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::PropertiesPanel;
use document::types::Color as AcadColor;
use document::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};

/// Run blocking work (disk, network) on a thread of its own: the executor
/// that delivers timers and control messages has one thread, and blocking it
/// freezes the app until the work ends. `None` when the worker panicked.
#[cfg(not(target_arch = "wasm32"))]
pub(in crate::app) async fn on_worker_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    receiver.await.ok()
}

/// Wait for save work without occupying the executor; a worker panic follows
/// save failure handling just like the former join error.
#[cfg(not(target_arch = "wasm32"))]
async fn native_save_worker<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, crate::io::SaveFailure> {
    on_worker_thread(work)
        .await
        .ok_or_else(|| crate::io::SaveFailure::other("save worker panicked"))
}

/// Write an exported file: to disk on the desktop, as a browser download on
/// the web, where there is no filesystem behind the dialog's path. (#761)
fn write_export(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    {
        let name = path
            .file_name()
            .map_or_else(|| "export".into(), |n| n.to_string_lossy());
        crate::sys::download_bytes(&name, bytes);
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }
}

/// `page` rendered `width` × `height` pixels on white, encoded as `format`.
pub(in crate::app) fn raster_page(
    page: &PdfPageInput,
    width: u32,
    height: u32,
    format: image::ImageFormat,
) -> Result<Vec<u8>, String> {
    let svg = crate::io::svg_export::build_svg(page, None)?;
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default())
        .map_err(|e| e.to_string())?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).ok_or("image size out of range")?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    let size = tree.size();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(
            width as f32 / size.width(),
            height as f32 / size.height(),
        ),
        &mut pixmap.as_mut(),
    );
    // Opaque, so the premultiplied pixels are the plain ones.
    let rgba = image::RgbaImage::from_raw(width, height, pixmap.take())
        .ok_or("image size out of range")?;
    let picture = match format {
        image::ImageFormat::Jpeg | image::ImageFormat::Bmp => {
            image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(rgba).to_rgb8())
        }
        _ => image::DynamicImage::ImageRgba8(rgba),
    };
    let mut bytes = Vec::new();
    picture
        .write_to(&mut std::io::Cursor::new(&mut bytes), format)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// Run `work` on a worker thread and map its result into a `Message`.
///
/// `context` names the operation for error reporting. A worker panic never
/// reaches the UI thread: it is caught on the worker and re-surfaced as
/// [`Message::BackgroundTaskFailed`] so a crashing export or import reports
/// on the command line instead of killing the application.
pub(in crate::app) fn background_task<T, F, M>(
    context: impl Into<String>,
    work: F,
    map: M,
) -> Task<Message>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
    M: FnOnce(T) -> Message + Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let context = context.into();
        let (tx, rx) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
            let _ = tx.send(result);
        });
        Task::perform(
            async move {
                match rx.await {
                    Ok(Ok(value)) => map(value),
                    Ok(Err(payload)) => Message::BackgroundTaskFailed {
                        context,
                        detail: panic_detail(payload),
                    },
                    Err(_) => Message::BackgroundTaskFailed {
                        context,
                        detail: "worker stopped without reporting a result".to_string(),
                    },
                }
            },
            |message| message,
        )
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = context;
        Task::perform(async move { work() }, map)
    }
}

/// Render a panic payload the way `std` reports it: `&'static str` messages,
/// `String` messages, or a fallback for foreign payloads.
#[cfg(not(target_arch = "wasm32"))]
pub(in crate::app) fn panic_detail(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&'static str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "worker panicked".to_string()
    }
}

use crate::io::pdf_export::{PdfPageInput, PlotContent};

mod settings;
mod open;
mod save;
mod autosave;
mod plot;

pub(crate) use self::settings::*;
pub(crate) use self::open::*;
pub(crate) use self::save::*;
pub(crate) use self::autosave::*;
pub(crate) use self::plot::*;

#[cfg(test)]
mod plot_owner_aci_tests;

impl OpenCADStudio {
    #[cfg(target_arch = "wasm32")]
    pub(super) fn poll_embed_bridge(&mut self) -> Task<Message> {
        use crate::io::host_bridge::{self, HostCommand};

        if !host_bridge::is_embed_mode() {
            return Task::none();
        }
        let Some(cmd) = host_bridge::take_command() else {
            return Task::none();
        };
        match cmd {
            HostCommand::Open {
                name,
                bytes,
                read_only,
            } => {
                if self.opening.is_some() {
                    host_bridge::requeue_front(HostCommand::Open {
                        name,
                        bytes,
                        read_only,
                    });
                    return Task::none();
                }
                for tab in &self.tabs {
                    if !tab.is_start {
                        crate::modules::draw::draw::attdef::session().forget_place_of(tab.id);
                        crate::modules::annotate::text::forget_place_of(tab.id);
                        crate::io::fs_access::drop_handle(tab.id);
                    }
                }
                self.tabs.retain(|t| t.is_start);
                if self.tabs.is_empty() {
                    self.tabs
                        .push(crate::app::tab_document::DocumentTab::new_start());
                }
                self.active_tab = 0;
                host_bridge::clear_pending_saves();
                self.read_only = read_only;
                self.active_modal = None;
                self.update(Message::WebFileDropped(name, bytes, None))
            }
            HostCommand::RequestSave { format } => {
                if self.read_only {
                    host_bridge::emit_error("read_only", "Read-only session: saving is disabled.");
                    return Task::none();
                }
                let Some(fmt) = format
                    .filter(|f| f.eq_ignore_ascii_case("dxf") || f.eq_ignore_ascii_case("dwg"))
                else {
                    return self.on_save_file();
                };
                if let Some(busy_tab) = crate::io::fs_access::web_save_in_flight() {
                    if self.tabs.iter().any(|tab| tab.id == busy_tab) {
                        self.command_line.push_info(
                            crate::t!("Save already running for this drawing.").as_ref(),
                        );
                        return Task::none();
                    }
                    crate::io::fs_access::end_web_save(busy_tab);
                }
                let Some(tab) = self.tabs.get(self.active_tab) else {
                    return Task::none();
                };
                let is_dxf = fmt.eq_ignore_ascii_case("dxf");
                let ext = if is_dxf { "dxf" } else { "dwg" };
                self.save_plain_reuse_fs_handle = false;
                self.save_dialog_format =
                    crate::io::format_for_version(tab.scene.document.version, is_dxf);
                let stem = tab
                    .current_path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| tab.tab_display_name());
                self.save_dialog_filename = format!("{stem}.{ext}");
                self.aec_drop_acknowledged = false;
                self.on_save_dialog_confirm()
            }
            HostCommand::SaveAck {
                save_id,
                ok,
                name,
                error,
            } => {
                let matched = host_bridge::take_pending_save(save_id.as_deref());
                if ok {
                    let saved_label = host_bridge::saved_label_from_ack(
                        name.as_deref(),
                        matched.as_ref().map(|p| p.filename.as_str()),
                    );
                    if let Some(p) = matched {
                        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == p.tab_id) {
                            if let Some(ref label) = saved_label {
                                tab.current_path = Some(std::path::PathBuf::from(label));
                            }
                            if tab.scene.geometry_epoch == p.geometry_epoch
                                && tab.edit_revision == p.edit_revision
                                && tab.scene.camera_generation == p.camera_generation
                            {
                                tab.dirty = false;
                                tab.scene.note_tables_saved();
                            }
                        }
                    }
                    host_bridge::emit_dirty(
                        self.tabs.get(self.active_tab).is_some_and(|t| t.dirty),
                    );
                    if let Some(filename) = saved_label {
                        self.command_line
                            .push_output(crate::tf!("Saved: {filename}").as_ref());
                    }
                } else {
                    if let Some(p) = matched {
                        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == p.tab_id) {
                            tab.dirty = true;
                        }
                    }
                    host_bridge::emit_dirty(
                        self.tabs.get(self.active_tab).is_some_and(|t| t.dirty),
                    );
                    let msg = error.as_deref().unwrap_or("Host save failed");
                    self.command_line.push_error(msg);
                    host_bridge::emit_error("host_save_failed", msg);
                }
                Task::none()
            }
        }
    }
}

#[cfg(test)]
mod plot_paper_tests;

#[cfg(test)]
mod plot_device_persistence_tests;

#[cfg(test)]
mod background_task_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod native_save_worker_tests;

#[cfg(test)]
mod plot_style_panel_tests;

impl OpenCADStudio {
    /// Web: one warning lists every unresolved xref and the way to fix it —
    /// dropping the files on the page registers their bytes, and reopening
    /// the drawing (or XRELOAD) then resolves them.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn warn_unresolved_xrefs(&mut self, infos: &[crate::io::xref::XrefInfo]) {
        let unresolved = crate::io::xref::unresolved_xref_names(infos);
        if unresolved.is_empty() {
            return;
        }
        let names = unresolved.join(", ");
        self.command_line.push_warning(
            crate::tf!(
                "XREF  Unresolved external references: {names} — drop the referenced DWG/DXF files on the page to register them, then reopen this drawing or run XRELOAD."
            )
            .as_ref(),
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod save_header_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod mtext_save_loss_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod external_change_reload_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod minimized_save_tests;

/// Folder of the session locks, one per running editor.
#[cfg(not(target_arch = "wasm32"))]
fn autosave_session_dir() -> Option<std::path::PathBuf> {
    crate::config::config_dir().map(|dir| dir.join("sessions"))
}

/// This editor's session id, part of its unsaved drawings' recovery copy
/// names. Its lock file is held from the first call until the process ends.
#[cfg(not(target_arch = "wasm32"))]
fn autosave_session() -> &'static str {
    static SESSION: std::sync::OnceLock<(String, Option<std::fs::File>)> =
        std::sync::OnceLock::new();
    &SESSION
        .get_or_init(|| {
            let started = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default();
            let id = format!("{:x}{started:x}", std::process::id());
            let lock = autosave_session_dir().and_then(|dir| {
                std::fs::create_dir_all(&dir).ok()?;
                let file = std::fs::File::create(dir.join(format!("{id}.lock"))).ok()?;
                file.try_lock().ok()?;
                Some(file)
            });
            (id, lock)
        })
        .0
}

/// Whether the editor of session `id` still runs: its lock is held. A
/// session found ended has its lock file removed.
#[cfg(not(target_arch = "wasm32"))]
fn session_running(id: &str) -> bool {
    if id.is_empty() || id == autosave_session() {
        return !id.is_empty();
    }
    let Some(path) = autosave_session_dir().map(|dir| dir.join(format!("{id}.lock"))) else {
        return false;
    };
    let Ok(file) = std::fs::File::open(&path) else {
        return false;
    };
    match file.try_lock() {
        Err(std::fs::TryLockError::WouldBlock) => true,
        _ => {
            drop(file);
            let _ = std::fs::remove_file(&path);
            false
        }
    }
}
