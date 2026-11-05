//! Tool dialogs that stay open beside the drawing, and windows of their own.
//!
//! Every dialog is a *tool*: it opens as a frame over the drawing that lets
//! the drawing, the command line and other dialogs work while it is open, and
//! on the desktop it — or any dock pallet — can move into a window of its
//! own, onto another monitor. The web build has one canvas, so there tools and
//! pallets stay frames inside it.
//!
//! Dialog code keys everything on `active_modal` (the one dialog its messages
//! and its close path work on). A tool keeps that code as it is: while one of
//! its own messages runs it is swapped in as the active modal, and once the
//! update is over it is parked again (`settle_tool_windows`). A tool that is
//! no longer the active modal after its message was closed or replaced.

use super::{Message, ModalKind, OpenCADStudio};
use iced::widget::{mouse_area, Space};
#[cfg(not(target_arch = "wasm32"))]
use iced::widget::{column, container, row, text};
use iced::{Element, Fill, Point, Size, Task, Vector};

/// Where a tool dialog's state lives, which decides what a tab switch does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolScope {
    /// Settings of the program: open whatever drawing is current.
    App,
    /// Works on the current drawing: closes when another drawing comes up.
    Document,
}

impl ModalKind {
    /// Where this dialog's state lives. Every dialog is a tool: program
    /// settings, notices and the flows that hold a file or a tab operation
    /// half done stay open on a drawing switch; the rest work on the current
    /// drawing and close with it.
    pub(crate) fn tool_scope(self) -> ToolScope {
        use ModalKind as K;
        match self {
            K::About
            | K::Shortcuts
            | K::Aliases
            | K::PluginManager
            | K::Options
            | K::XrefHelp
            | K::UpdateNotice
            | K::DonationPrompt
            | K::GpuWarning
            | K::AssocPrompt
            | K::Unsaved
            | K::BlockEditChanges
            | K::SaveDialog
            | K::Recovery
            | K::RecoveryPrompt
            | K::ConversionLoss
            | K::AecDropWarning
            | K::ImportFile
            | K::DgnImport
            | K::TdsImport
            | K::TdsMaterial
            | K::TdsMissingMap
            | K::PdfImportFile
            | K::PdfPassword
            | K::SheetSet
            | K::PluginDialog
            | K::ExtractionOutdated
            | K::DataExtractionUnsaved => ToolScope::App,
            #[cfg(not(target_arch = "wasm32"))]
            K::FileInUse | K::ExternalChange => ToolScope::App,
            #[cfg(target_arch = "wasm32")]
            K::AutosaveRestore => ToolScope::App,
            _ => ToolScope::Document,
        }
    }
}

/// One open tool dialog.
#[derive(Debug, Clone)]
pub(crate) struct ToolWindow {
    pub kind: ModalKind,
    /// The drawing it opened on (the tab id).
    pub tab: Option<u64>,
    /// Shift from the centre of the drawing area, as a modal's.
    pub offset: Vector,
    /// Growth past the dialog's natural size (the resize grip).
    pub resize: Vector,
    /// The content's measured size, for clamping and its own window's size.
    pub content_size: Option<Size>,
    /// Desktop: shown in a window of its own (`Some` once it is open).
    #[cfg(not(target_arch = "wasm32"))]
    pub window: Option<iced::window::Id>,
    /// Desktop: asked to go into (or reopen in) a window of its own.
    #[cfg(not(target_arch = "wasm32"))]
    pub wants_window: bool,
}

impl ToolWindow {
    pub(crate) fn in_canvas(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.window.is_none() && !self.wants_window
        }
        #[cfg(target_arch = "wasm32")]
        {
            true
        }
    }
}

/// A tool frame being dragged or resized.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ToolDrag {
    pub kind: ModalKind,
    pub resizing: bool,
    pub last: Option<Point>,
}

/// Where a window of its own last was, in screen logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WindowPlacement {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// What a window of its own shows.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Detached {
    Tool(ModalKind),
    Panel(crate::ui::dock::PanelId),
}

/// A window of its own (desktop).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct OsWindow {
    pub id: iced::window::Id,
    pub content: Detached,
    pub placement: WindowPlacement,
}

/// The key a tool dialog's placement is remembered under.
#[cfg(not(target_arch = "wasm32"))]
fn tool_key(kind: ModalKind) -> String {
    format!("{kind:?}")
}

/// The key a pallet's window placement is remembered under, beside the
/// dialogs' ones.
#[cfg(not(target_arch = "wasm32"))]
fn pallet_key(id: crate::ui::dock::PanelId) -> String {
    format!("pallet:{id:?}")
}

/// Frame padding and title bar around a dialog's content, as `modal` draws it.
const FRAME_W: f32 = 20.0;
const FRAME_H: f32 = 50.0;
/// How far each further tool opens from the last, so none hides another.
const CASCADE: f32 = 28.0;

#[cfg(not(target_arch = "wasm32"))]
static MAIN_WINDOW: std::sync::Mutex<Option<iced::window::Id>> = std::sync::Mutex::new(None);

/// Desktop: remember the main window so the event subscription can tell its
/// keys from those typed into a window of its own.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn set_main_window(id: iced::window::Id) {
    if let Ok(mut main) = MAIN_WINDOW.lock() {
        *main = Some(id);
    }
}

/// Whether `id` is the main window (or no main window is known yet). Keys
/// typed in a window of its own stay with that window's widgets: they do not
/// reach the command line or the drawing's shortcuts.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn is_main_window(id: iced::window::Id) -> bool {
    MAIN_WINDOW
        .lock()
        .map(|main| main.is_none_or(|main| main == id))
        .unwrap_or(true)
}

/// The redraws of the main window, for the subscriptions that tick until the
/// drawing has drawn something. `window::frames` follows every window: with
/// a dialog in a window of its own, each of its frames ticks them, each tick
/// redraws every window, the dialog's window is painted first again and the
/// main window — the one they wait for — never paints, so they never stop
/// and no new dialog is ever measured for its window.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn main_window_frames() -> iced::Subscription<iced::time::Instant> {
    iced::event::listen_raw(|event, _status, id| match event {
        iced::Event::Window(iced::window::Event::RedrawRequested(at)) if is_main_window(id) => {
            Some(at)
        }
        _ => None,
    })
}

/// The web build has one window.
#[cfg(target_arch = "wasm32")]
pub(crate) fn main_window_frames() -> iced::Subscription<iced::time::Instant> {
    iced::window::frames()
}

impl OpenCADStudio {
    /// The update iced runs: one message, then open tools parked again.
    pub fn update_root(&mut self, msg: Message) -> Task<Message> {
        let msg = self.route_tool_keyboard(msg);
        let task = self.update(msg);
        let settled = self.settle_tool_windows();
        Task::batch([task, settled])
    }

    /// Whether the active modal may park: the colour picker is drawn over
    /// the dialog it was opened from and answers into it, so while it is up
    /// that dialog stays modal.
    fn tool_parkable(&self) -> bool {
        self.color_picker.is_none()
    }

    /// Whether a tool dialog of `kind` is open, active or parked.
    pub(crate) fn tool_open(&self, kind: ModalKind) -> bool {
        self.tool_windows.iter().any(|tool| tool.kind == kind)
    }

    /// Make parked tool `kind` the active modal for the message about to run.
    /// Refused while a modal dialog (one that is not a tool) is up.
    pub(crate) fn enter_tool(&mut self, kind: ModalKind) -> bool {
        // A tool swapped in earlier in this update and closed since is gone.
        if let Some(focus) = self.tool_focus {
            if self.active_modal != Some(focus) {
                self.tool_windows.retain(|tool| tool.kind != focus);
                self.tool_focus = None;
            }
        }
        match self.active_modal {
            Some(active) if active == kind => return true,
            Some(active) if !self.tool_open(active) && !self.tool_parkable() => return false,
            Some(_) => self.park_active_tool(),
            None => {}
        }
        // Swapping in keeps the stacking order: only a press raises a tool.
        let Some(tool) = self.tool_windows.iter().find(|tool| tool.kind == kind) else {
            return false;
        };
        self.modal_offset = tool.offset;
        self.modal_resize = tool.resize;
        self.modal_content_size = tool.content_size;
        self.modal_drag_last = None;
        self.modal_dragging = false;
        self.modal_resizing = false;
        self.active_modal = Some(kind);
        self.tool_focus = Some(kind);
        true
    }

    /// Park the active modal, a tool, keeping its frame where it is.
    fn park_active_tool(&mut self) {
        let Some(kind) = self.active_modal else {
            return;
        };
        let geometry = (self.modal_offset, self.modal_resize, self.modal_content_size);
        let cascade = self.tool_windows.iter().filter(|tool| tool.in_canvas()).count() as f32;
        match self.tool_windows.iter().position(|tool| tool.kind == kind) {
            // Swapped in for a message: it stays where it was moved to.
            Some(index) if self.tool_focus == Some(kind) => {
                let tool = &mut self.tool_windows[index];
                (tool.offset, tool.resize, tool.content_size) = geometry;
            }
            // Opened again while open: it comes to the front where it is.
            Some(index) => {
                let tool = self.tool_windows.remove(index);
                self.tool_windows.push(tool);
                self.keyboard_tool = Some(kind);
            }
            None => {
                // A new tool: cascade it from the ones already open, and back
                // into its own window when it was in one when last closed.
                let offset = if geometry.0 == Vector::ZERO {
                    Vector::new(CASCADE * cascade, CASCADE * cascade)
                } else {
                    geometry.0
                };
                // Desktop: a dialog opens in a window of its own.
                #[cfg(not(target_arch = "wasm32"))]
                let wants_window = self.main_window.is_some();
                // A dialog that just opened has the keyboard, as a modal had.
                self.keyboard_tool = Some(kind);
                let tab = self.tabs.get(self.active_tab).map(|tab| tab.id);
                self.tool_windows.push(ToolWindow {
                    kind,
                    tab,
                    offset,
                    resize: geometry.1,
                    content_size: geometry.2,
                    #[cfg(not(target_arch = "wasm32"))]
                    window: None,
                    #[cfg(not(target_arch = "wasm32"))]
                    wants_window,
                });
            }
        }
        self.active_modal = None;
        self.modal_offset = Vector::ZERO;
        self.modal_resize = Vector::ZERO;
        self.modal_content_size = None;
        self.modal_drag_last = None;
        self.modal_dragging = false;
        self.modal_resizing = false;
    }

    /// After every update: a tool swapped in and no longer active was closed
    /// (or replaced by the dialog it opened); a tool left active parks. On
    /// the desktop the windows of their own follow what is open.
    pub(crate) fn settle_tool_windows(&mut self) -> Task<Message> {
        if let Some(kind) = self.tool_focus {
            if self.active_modal != Some(kind) {
                self.tool_windows.retain(|tool| tool.kind != kind);
                self.tool_focus = None;
            }
        }
        // However the drawing changed (a switch, a close, a discard), tools
        // working on the one that went close with it.
        let tab = self.tabs.get(self.active_tab).map(|tab| tab.id);
        if self
            .tool_windows
            .iter()
            .any(|tool| tool.kind.tool_scope() == ToolScope::Document && tool.tab != tab)
        {
            self.close_document_tools(tab);
        }
        if self.active_modal.is_some() && self.tool_parkable() {
            self.park_active_tool();
        }
        self.tool_focus = None;
        if self
            .keyboard_tool
            .is_some_and(|kind| !self.tool_windows.iter().any(|tool| tool.kind == kind && tool.in_canvas()))
        {
            self.keyboard_tool = None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.reconcile_os_windows()
        }
        #[cfg(target_arch = "wasm32")]
        {
            Task::none()
        }
    }

    /// A message from tool `kind`'s frame or window: it runs with the tool as
    /// the active modal. Ignored while a modal dialog is up — that dialog
    /// blocks the drawing and every tool until it is answered.
    pub(crate) fn on_tool_window_msg(&mut self, kind: ModalKind, inner: Message) -> Task<Message> {
        if !self.enter_tool(kind) {
            return Task::none();
        }
        // What the message starts (a file read, a save, a pick) answers to
        // the same dialog, as it did while that dialog was modal.
        self.update(inner)
            .map(move |msg| Message::ToolWindowTask(kind, Box::new(msg)))
    }

    /// The answer of a task tool `kind` started: it runs with the tool swapped
    /// in while it is still open, and as a plain message once it is not.
    pub(crate) fn on_tool_window_task(&mut self, kind: ModalKind, inner: Message) -> Task<Message> {
        if self.tool_open(kind) {
            self.on_tool_window_msg(kind, inner)
        } else {
            self.update(inner)
        }
    }

    /// Keys while a dialog has the keyboard go to it, as they went to a modal
    /// dialog: Esc closes it, Enter answers it, the rest stay out of the
    /// command line. A press on the drawing, a pane, the ribbon or into the
    /// command line gives the keyboard back to the drawing.
    fn route_tool_keyboard(&mut self, msg: Message) -> Message {
        if matches!(
            msg,
            Message::ViewportLeftPress
                | Message::ViewportLeftPressAt(_)
                | Message::ViewportRightPress
                | Message::ViewportMiddlePress
                | Message::PaneClicked(_)
                | Message::PanePress(_)
                | Message::PanePressAt(..)
                | Message::PaneRightPress(_)
                | Message::PaneMiddlePress(_)
                | Message::RibbonToolClick { .. }
                | Message::CommandInput(_)
        ) {
            self.keyboard_tool = None;
        }
        match self.keyboard_tool {
            Some(kind)
                if self.active_modal.is_none()
                    && (matches!(msg, Message::CommandEscape)
                        || super::update::is_modal_blocked_key_msg(&msg)) =>
            {
                Message::ToolWindow(kind, Box::new(msg))
            }
            _ => msg,
        }
    }

    /// Automation: drive dialog `kind` whether it is the modal one or an open
    /// tool (which becomes the active modal for this request).
    pub(crate) fn focus_dialog(&mut self, kind: ModalKind) -> bool {
        self.active_modal == Some(kind) || (self.tool_open(kind) && self.enter_tool(kind))
    }

    /// A manager pallet called up the way a dialog is (its command, a ribbon
    /// button, a menu entry): on the desktop it opens in a window of its own,
    /// on the web floating over the drawing like a dialog. One already
    /// showing stays where it is; the dock's own menu opens it as a pallet.
    pub(crate) fn pallet_as_window(&mut self, id: crate::ui::dock::PanelId) {
        if self.dock_panel_wanted(id) {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.main_window.is_some() && !self.dock.is_separate(id) {
            let _ = self.detach_panel(id);
        }
        #[cfg(target_arch = "wasm32")]
        self.float_pallet_centred(id);
    }

    /// Float pallet `id` over the middle of the drawing (where it floated
    /// last, when it did), out of the dock.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn float_pallet_centred(&mut self, id: crate::ui::dock::PanelId) {
        if self.dock.float_rect(id).is_none() {
            let (ww, wh) = self.dock_workspace_size();
            let (w, h) = self.dock_float_size(id);
            let (x, y) = (((ww - w) * 0.5).max(0.0), ((wh - h) * 0.5).max(0.0));
            self.dock
                .float(crate::ui::dock::FloatPanel::new(id, x, y, w, h));
            self.save_config();
        }
        self.dock.raise_float(id);
    }

    /// Whether dialog `kind` is up, the active modal or an open tool.
    pub(crate) fn dialog_open(&self, kind: ModalKind) -> bool {
        self.active_modal == Some(kind) || self.tool_open(kind)
    }

    /// The drawing changed: tools working on it close, as their modal forms
    /// did when the drawing could not change under them. Those opened on
    /// tab `keep` stay.
    pub(crate) fn close_document_tools(&mut self, keep: Option<u64>) {
        let kinds: Vec<ModalKind> = self
            .tool_windows
            .iter()
            .filter(|tool| tool.kind.tool_scope() == ToolScope::Document && tool.tab != keep)
            .map(|tool| tool.kind)
            .collect();
        for kind in kinds {
            if self.enter_tool(kind) {
                self.close_active_modal();
                if self.active_modal == Some(kind) {
                    self.active_modal = None;
                }
                self.tool_focus = None;
            }
            self.tool_windows.retain(|tool| tool.kind != kind);
        }
    }

    pub(crate) fn on_tool_grab(&mut self, kind: ModalKind, resizing: bool) -> Task<Message> {
        if self.active_modal.is_some_and(|active| !self.tool_open(active)) {
            return Task::none();
        }
        if let Some(index) = self.tool_windows.iter().position(|tool| tool.kind == kind) {
            let tool = self.tool_windows.remove(index);
            self.tool_windows.push(tool);
        }
        self.tool_drag = Some(ToolDrag {
            kind,
            resizing,
            last: None,
        });
        Task::none()
    }

    pub(crate) fn on_tool_drag_move(&mut self, point: Point) -> Task<Message> {
        let Some(drag) = self.tool_drag.as_mut() else {
            return Task::none();
        };
        let last = drag.last.replace(point);
        let (kind, resizing) = (drag.kind, drag.resizing);
        let Some(last) = last else {
            return Task::none();
        };
        let (dx, dy) = (point.x - last.x, point.y - last.y);
        let (ww, wh) = self.win_size;
        let Some(tool) = self.tool_windows.iter_mut().find(|tool| tool.kind == kind) else {
            return Task::none();
        };
        if resizing {
            // Same rule as a modal's grip: the natural size is the floor, and
            // the centre moves half the growth so the top-left corner stays.
            let nx = (tool.resize.x + dx).max(0.0);
            let ny = (tool.resize.y + dy).max(0.0);
            tool.offset.x += (nx - tool.resize.x) * 0.5;
            tool.offset.y += (ny - tool.resize.y) * 0.5;
            tool.resize = Vector::new(nx, ny);
        } else {
            tool.offset.x += dx;
            tool.offset.y += dy;
            if let Some(size) = tool.content_size {
                let max_x = ((ww - size.width - FRAME_W) * 0.5).max(0.0);
                let max_y = ((wh - size.height - FRAME_H) * 0.5).max(0.0);
                tool.offset.x = tool.offset.x.clamp(-max_x, max_x);
                tool.offset.y = tool.offset.y.clamp(-max_y, max_y);
            }
        }
        Task::none()
    }

    pub(crate) fn on_tool_drag_release(&mut self) -> Task<Message> {
        self.tool_drag = None;
        Task::none()
    }

    /// The open tools' frames over `base`, back to front, plus the pointer
    /// capture of a drag. Tools in a window of their own are drawn there.
    pub(crate) fn with_tool_windows<'a>(&'a self, base: Element<'a, Message>) -> Element<'a, Message> {
        let mut layers: Vec<Element<'a, Message>> = vec![base];
        for tool in self.tool_windows.iter().filter(|tool| tool.in_canvas()) {
            let kind = tool.kind;
            let Some(content) = self.modal_content_for(kind, tool.resize) else {
                continue;
            };
            let content = content.map(move |msg| Message::ToolWindow(kind, Box::new(msg)));
            layers.push(crate::ui::modal::tool_frame(
                self.modal_title_for(kind),
                content,
                tool.offset,
                crate::ui::modal::ToolFrameMessages {
                    close: Message::ToolWindow(kind, Box::new(Message::CloseModal)),
                    grab: Message::ToolWindowGrab(kind),
                    resize: Message::ToolWindowResizeGrab(kind),
                    raise: Message::ToolWindowRaise(kind),
                    measured: Box::new(move |size| {
                        Message::ToolWindow(kind, Box::new(Message::ModalContentResized(size)))
                    }),
                },
            ));
        }
        // A dialog bound for a window of its own is laid out once off screen,
        // so its window opens at the size it needs.
        #[cfg(not(target_arch = "wasm32"))]
        for tool in self.tool_windows.iter().filter(|tool| {
            tool.wants_window && tool.window.is_none() && tool.content_size.is_none()
        }) {
            let kind = tool.kind;
            let Some(content) = self.modal_content_for(kind, Vector::ZERO) else {
                continue;
            };
            let (ww, wh) = self.win_size;
            let measured = iced::widget::sensor(
                content.map(move |msg| Message::ToolWindow(kind, Box::new(msg))),
            )
            .on_resize(move |size| {
                Message::ToolWindow(kind, Box::new(Message::ModalContentResized(size)))
            });
            layers.push(
                // Laid out in the window's size, drawn past its corner.
                iced::widget::pin(iced::widget::pin(measured).width(ww).height(wh))
                    .x(-(ww + 1000.0))
                    .y(-(wh + 1000.0))
                    .into(),
            );
        }
        if let Some(drag) = self.tool_drag {
            layers.push(
                mouse_area(Space::new().width(Fill).height(Fill))
                    .on_move(Message::ToolWindowDragMove)
                    .on_release(Message::ToolWindowDragRelease)
                    .interaction(if drag.resizing {
                        iced::mouse::Interaction::ResizingDiagonallyDown
                    } else {
                        iced::mouse::Interaction::Grabbing
                    })
                    .into(),
            );
        }
        if layers.len() == 1 {
            return layers.pop().expect("the base layer");
        }
        iced::widget::Stack::with_children(layers)
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// Raise tool `kind` to the front.
    pub(crate) fn raise_tool(&mut self, kind: ModalKind) -> Task<Message> {
        if let Some(index) = self.tool_windows.iter().position(|tool| tool.kind == kind) {
            let tool = self.tool_windows.remove(index);
            self.tool_windows.push(tool);
        }
        Task::none()
    }
}

// ── Windows of their own (desktop) ──────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
impl OpenCADStudio {
    /// Move dock pallet `id` into a window of its own.
    pub(crate) fn detach_panel(&mut self, id: crate::ui::dock::PanelId) -> Task<Message> {
        // A pallet floating short in the main window still opens a window
        // tall enough to work in.
        let (w, h) = self
            .dock
            .float_rect(id)
            .map(|f| (f.w, f.h))
            .unwrap_or_else(|| self.dock_float_size(id));
        let h = h.max(PALLET_WINDOW_MIN_H);
        let placement = self
            .dock
            .detached
            .get(&id)
            .or_else(|| self.dock.tool_windows.get(&pallet_key(id)))
            .copied()
            .unwrap_or(WindowPlacement {
                x: f32::NAN,
                y: f32::NAN,
                w,
                h: h + HEADER_H,
            });
        self.dock.separate(id, placement);
        self.dock_float_menu = None;
        self.save_config();
        Task::none()
    }

    /// A pallet window's "back to the main window" button: it floats in the
    /// main window again.
    pub(crate) fn return_os_window(&mut self, id: iced::window::Id) -> Task<Message> {
        let Some(window) = self.os_windows.iter().find(|window| window.id == id).copied() else {
            return Task::none();
        };
        let Detached::Panel(panel) = window.content else {
            return Task::none();
        };
        self.dock.detached.remove(&panel);
        let (ww, wh) = self.dock_workspace_size();
        let (w, h) = (window.placement.w, (window.placement.h - HEADER_H).max(160.0));
        let (x, y) = (((ww - w) * 0.5).max(0.0), ((wh - h) * 0.5).max(0.0));
        self.dock
            .float(crate::ui::dock::FloatPanel::new(panel, x, y, w, h));
        self.dock.raise_float(panel);
        self.os_windows.retain(|window| window.id != id);
        self.save_config();
        iced::window::close(id)
    }

    /// The window manager's close button on a window of its own: a tool
    /// closes (its next open comes back in a window), a pallet hides.
    pub(crate) fn os_window_close_requested(&mut self, id: iced::window::Id) -> Task<Message> {
        let Some(window) = self.os_windows.iter().find(|window| window.id == id).copied() else {
            return iced::window::close(id);
        };
        let task = match window.content {
            Detached::Tool(kind) => {
                if self.enter_tool(kind) {
                    self.update(Message::CloseModal)
                } else {
                    Task::none()
                }
            }
            Detached::Panel(panel) => self.dock_set_open(panel, false),
        };
        self.os_windows.retain(|window| window.id != id);
        self.save_config();
        Task::batch([task, iced::window::close(id)])
    }

    /// Esc or Enter in a window of its own: its dialog answers it, as it
    /// answered them while modal.
    pub(crate) fn os_window_key(&mut self, id: iced::window::Id, key: String) -> Task<Message> {
        match window_content(&self.os_windows, id) {
            Some(Detached::Tool(kind)) => {
                self.on_tool_window_msg(kind, Message::ShortcutPressed(key))
            }
            _ => Task::none(),
        }
    }

    /// A window of its own went away (closed by us or by the system).
    pub(crate) fn os_window_closed(&mut self, id: iced::window::Id) {
        self.os_windows.retain(|window| window.id != id);
        for tool in &mut self.tool_windows {
            if tool.window == Some(id) {
                tool.window = None;
            }
        }
    }

    pub(crate) fn os_window_resized(&mut self, id: iced::window::Id, w: f32, h: f32) {
        if let Some(window) = self.os_windows.iter_mut().find(|window| window.id == id) {
            window.placement.w = w;
            window.placement.h = h;
            let placement = window.placement;
            self.remember_placement(window_content(&self.os_windows, id), placement);
        }
    }

    pub(crate) fn os_window_moved(&mut self, id: iced::window::Id, at: Point) {
        if let Some(window) = self.os_windows.iter_mut().find(|window| window.id == id) {
            window.placement.x = at.x;
            window.placement.y = at.y;
            let placement = window.placement;
            self.remember_placement(window_content(&self.os_windows, id), placement);
        }
    }

    fn remember_placement(&mut self, content: Option<Detached>, placement: WindowPlacement) {
        match content {
            Some(Detached::Tool(kind)) => {
                self.dock.tool_windows.insert(tool_key(kind), placement);
            }
            Some(Detached::Panel(panel)) => {
                self.dock.detached.insert(panel, placement);
                // Kept after it comes back, for the next time it is called
                // up as a window.
                self.dock.tool_windows.insert(pallet_key(panel), placement);
            }
            None => {}
        }
    }

    /// Open the windows that should be up and close the ones that should not:
    /// a tool asked into a window, a detached pallet that is open; a window
    /// whose tool closed or whose pallet hid or came back into the dock.
    fn reconcile_os_windows(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        // Close what lost its reason to be open.
        let stale: Vec<iced::window::Id> = self
            .os_windows
            .iter()
            .filter(|window| match window.content {
                Detached::Tool(kind) => !self
                    .tool_windows
                    .iter()
                    .any(|tool| tool.kind == kind && tool.wants_window),
                Detached::Panel(panel) => {
                    !self.dock.detached.contains_key(&panel) || !self.dock_panel_wanted(panel)
                }
            })
            .map(|window| window.id)
            .collect();
        for id in stale {
            self.os_windows.retain(|window| window.id != id);
            for tool in &mut self.tool_windows {
                if tool.window == Some(id) {
                    tool.window = None;
                }
            }
            tasks.push(iced::window::close(id));
        }
        if self.main_window.is_none() {
            return Task::batch(tasks);
        }
        // Open what should be up.
        // A dialog's window opens once its content was measured (off screen,
        // see `with_tool_windows`), at the size it needs.
        let tools: Vec<(ModalKind, Size, Vector)> = self
            .tool_windows
            .iter()
            .filter(|tool| tool.wants_window && tool.window.is_none())
            .filter_map(|tool| Some((tool.kind, tool.content_size?, tool.resize)))
            .collect();
        for (kind, size, resize) in tools {
            let placement = self.dock.tool_windows.get(&tool_key(kind)).copied().unwrap_or(
                WindowPlacement {
                    x: f32::NAN,
                    y: f32::NAN,
                    // Whole pixels and a little slack: text laid out a
                    // fraction narrower than measured wraps a line lower.
                    w: size.width.ceil() + 2.0 * WINDOW_PAD + WINDOW_SLACK + resize.x,
                    h: size.height.ceil() + 2.0 * WINDOW_PAD + WINDOW_SLACK + resize.y,
                },
            );
            let (id, open) = open_window(placement, self.modal_title_for(kind));
            if let Some(tool) = self.tool_windows.iter_mut().find(|tool| tool.kind == kind) {
                tool.window = Some(id);
            }
            self.os_windows.push(OsWindow {
                id,
                content: Detached::Tool(kind),
                placement,
            });
            tasks.push(open);
        }
        let panels: Vec<(crate::ui::dock::PanelId, WindowPlacement)> = self
            .dock
            .detached
            .iter()
            .filter(|(panel, _)| {
                self.dock_panel_wanted(**panel)
                    && !self
                        .os_windows
                        .iter()
                        .any(|window| window.content == Detached::Panel(**panel))
            })
            .map(|(panel, placement)| (*panel, *placement))
            .collect();
        for (panel, placement) in panels {
            let (id, open) = open_window(placement, panel.title().to_string());
            self.os_windows.push(OsWindow {
                id,
                content: Detached::Panel(panel),
                placement,
            });
            tasks.push(open);
        }
        Task::batch(tasks)
    }

    /// The title of window `id`.
    pub(crate) fn os_window_title(&self, id: iced::window::Id) -> Option<String> {
        let window = self.os_windows.iter().find(|window| window.id == id)?;
        Some(match window.content {
            Detached::Tool(kind) => self.modal_title_for(kind),
            Detached::Panel(panel) => crate::t!(panel.title()).into_owned(),
        })
    }

    /// What window `id` shows; `None` for the main window.
    pub(crate) fn os_window_view(&self, id: iced::window::Id) -> Option<Element<'_, Message>> {
        let window = self.os_windows.iter().find(|window| window.id == id)?;
        // A pallet's window has "back to the main window"; a dialog's window
        // is only the dialog.
        let back = iced::widget::button(
            row![
                crate::ui::icons::themed_secondary(crate::ui::icons::DOCK_BACK, 12.0),
                text(crate::t!("Back to the main window")).size(11),
            ]
            .spacing(5)
            .align_y(iced::Center),
        )
        .on_press(Message::OsWindowReturn(id))
        .style(iced::widget::button::text)
        .padding([3, 8]);
        let back = container(back).id(iced::widget::Id::new(BACK_BUTTON_ID));
        let header = container(row![Space::new().width(Fill), back].align_y(iced::Center))
            .width(Fill)
            .height(iced::Length::Fixed(HEADER_H));
        let body: Element<'_, Message> = match window.content {
            Detached::Tool(kind) => {
                let tool = self.tool_windows.iter().find(|tool| tool.kind == kind)?;
                // The dialog grows with its window, past its natural size.
                let natural = tool.content_size.unwrap_or(Size::ZERO);
                let extra = Vector::new(
                    (window.placement.w - natural.width.ceil() - 2.0 * WINDOW_PAD - WINDOW_SLACK)
                        .max(0.0),
                    (window.placement.h
                        - natural.height.ceil()
                        - 2.0 * WINDOW_PAD
                        - WINDOW_SLACK)
                        .max(0.0),
                );
                let content = self.modal_content_for(kind, extra)?;
                let body: Element<'_, Message> =
                    container(content.map(move |msg| Message::ToolWindow(kind, Box::new(msg))))
                        .padding(WINDOW_PAD)
                        .center_x(Fill)
                        .into();
                // The colour picker a dialog opened is drawn over that dialog.
                match self.color_picker.as_ref() {
                    Some(picker)
                        if self.active_modal == Some(kind) && picker.eyedropper_view().is_none() =>
                    {
                        crate::ui::modal::modal(
                            body,
                            crate::t!("Select Color"),
                            picker.view(),
                            Message::CloseColorPicker,
                            Vector::ZERO,
                            crate::ui::modal::ModalOptions::NOTICE,
                        )
                    }
                    _ => body,
                }
            }
            Detached::Panel(panel) => {
                let tab = self.tabs.get(self.active_tab)?;
                column![header, self.panel_body(panel, window.placement.w, true, tab)].into()
            }
        };
        Some(
            container(body)
                .width(Fill)
                .height(Fill)
                .style(|theme: &iced::Theme| container::Style {
                    background: Some(iced::Background::Color(
                        theme.palette().background.base.color,
                    )),
                    ..Default::default()
                })
                .into(),
        )
    }
}

/// A window of its own's "back to the main window" button.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const BACK_BUTTON_ID: &str = "back-to-main-window";

/// The header strip of a window of its own.
#[cfg(not(target_arch = "wasm32"))]
const HEADER_H: f32 = 26.0;

/// The least height a pallet's window opens at.
#[cfg(not(target_arch = "wasm32"))]
const PALLET_WINDOW_MIN_H: f32 = 420.0;

/// Padding around a dialog in a window of its own.
#[cfg(not(target_arch = "wasm32"))]
const WINDOW_PAD: f32 = 10.0;

/// Room a dialog's window gets past its measured content.
#[cfg(not(target_arch = "wasm32"))]
const WINDOW_SLACK: f32 = 8.0;

#[cfg(not(target_arch = "wasm32"))]
fn window_content(windows: &[OsWindow], id: iced::window::Id) -> Option<Detached> {
    windows.iter().find(|window| window.id == id).map(|window| window.content)
}

/// Open a window of its own at `placement` (centred when it has no place).
#[cfg(not(target_arch = "wasm32"))]
fn open_window(placement: WindowPlacement, _title: String) -> (iced::window::Id, Task<Message>) {
    let position = if placement.x.is_finite() && placement.y.is_finite() {
        iced::window::Position::Specific(Point::new(placement.x, placement.y))
    } else {
        iced::window::Position::Centered
    };
    let (id, open) = iced::window::open(iced::window::Settings {
        size: Size::new(placement.w.max(240.0), placement.h.max(160.0)),
        position,
        min_size: Some(Size::new(240.0, 160.0)),
        exit_on_close_request: false,
        icon: super::helpers::build_window_icon()
            .and_then(|rgba| iced::window::icon::from_rgba(rgba, 32, 32).ok()),
        // The same app id as the main window: the dock groups them and shows
        // the app's icon.
        #[cfg(target_os = "linux")]
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: crate::io::file_association::APP_ID.to_string(),
            ..Default::default()
        },
        ..iced::window::Settings::default()
    });
    (id, open.map(|_| Message::Noop))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The messages a press on widget `id` in `element` sends.
    #[cfg(not(target_arch = "wasm32"))]
    fn press(element: Element<'_, Message>, id: &'static str) -> Vec<Message> {
        let mut ui = iced_test::simulator::Simulator::with_size(
            iced::Settings::default(),
            (1400.0, 900.0),
            element,
        );
        ui.click(iced::widget::Id::new(id)).expect("the button is drawn");
        ui.into_messages().collect()
    }

    fn press_text(element: Element<'_, Message>, label: &'static str) -> Vec<Message> {
        let mut ui = iced_test::simulator::Simulator::with_size(
            iced::Settings::default(),
            (1400.0, 900.0),
            element,
        );
        ui.click(label).expect("the button is drawn");
        ui.into_messages().collect()
    }

    /// Press the button labelled `label` in the open tool frames and run
    /// what it sends, as iced does.
    fn press_in_tools(app: &mut OpenCADStudio, label: &'static str) {
        let sent = press_text(app.with_tool_windows(Space::new().into()), label);
        assert!(!sent.is_empty(), "{label} sent nothing");
        for msg in sent {
            let _ = app.update_root(msg);
        }
    }

    /// A real key press, through the app's own event listener.
    #[cfg(not(target_arch = "wasm32"))]
    fn key(app: &mut OpenCADStudio, key: iced::keyboard::Key, text: &str) {
        use iced::keyboard::{key::NativeCode, key::Physical, Event, Location, Modifiers};
        let main = MAIN_WINDOW
            .lock()
            .ok()
            .and_then(|main| *main)
            .unwrap_or_else(iced::window::Id::unique);
        let event = iced::Event::Keyboard(Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: (!text.is_empty()).then(|| text.into()),
            repeat: false,
        });
        if let Some(msg) = crate::app::view::main_window_event(event, iced::event::Status::Ignored, main) {
            let _ = app.update_root(msg);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn keys_go_to_the_dialog_that_has_the_keyboard() {
        use iced::keyboard::{key::Named, Key};
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.automation_op(r#"{"op":"run","cmd":"BLOCK"}"#);
        let _ = app.update_root(Message::Noop);
        assert!(app.tool_open(ModalKind::BlockDefinition) && app.active_modal.is_none());
        // Typing stays out of the command line behind it.
        key(&mut app, Key::Character("l".into()), "l");
        assert_eq!(app.command_line.input, "");
        // Enter answers it: no name yet, so it says so and stays.
        key(&mut app, Key::Named(Named::Enter), "\r");
        let state = app.block_definition.as_ref().expect("still open");
        assert!(state.error_message.is_some(), "Enter pressed OK");
        assert!(app.tool_open(ModalKind::BlockDefinition));
        // A press on the drawing gives the keyboard back: typing reaches the
        // command line and Esc does not close the dialog.
        let _ = app.update_root(Message::ViewportLeftPress);
        key(&mut app, Key::Character("l".into()), "l");
        assert_eq!(app.command_line.input, "l");
        key(&mut app, Key::Named(Named::Escape), "");
        assert!(app.tool_open(ModalKind::BlockDefinition));
        // A press on its frame takes it again: Esc first dismisses its
        // warning, then closes it.
        let _ = app.update_root(Message::ToolWindowRaise(ModalKind::BlockDefinition));
        key(&mut app, Key::Named(Named::Escape), "");
        assert!(app.block_definition.as_ref().unwrap().error_message.is_none());
        assert!(app.tool_open(ModalKind::BlockDefinition));
        key(&mut app, Key::Named(Named::Escape), "");
        assert!(!app.tool_open(ModalKind::BlockDefinition));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_dialog_that_picks_on_the_drawing_comes_back_with_the_pick() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.add_entity(codec::EntityType::Line(
            codec::entities::Line::from_points(
                codec::types::Vector3::ZERO,
                codec::types::Vector3::UNIT_X,
            ),
        ));
        app.automation_op(r#"{"op":"run","cmd":"BLOCK"}"#);
        let _ = app.update_root(Message::Noop);
        // Its own "Select objects" button: the frame steps aside for the pick.
        press_in_tools(&mut app, "Select objects");
        assert!(!app.tool_open(ModalKind::BlockDefinition));
        assert!(app.tabs[app.active_tab].active_cmd.is_some(), "picking");
        // Answered on the command line, with real keys: the keyboard left
        // with the frame.
        for ch in ["A", "L", "L"] {
            key(&mut app, iced::keyboard::Key::Character(ch.into()), ch);
        }
        // Enter takes "ALL", the next ends the selection.
        for _ in 0..2 {
            key(&mut app, iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter), "\r");
        }
        assert!(app.tool_open(ModalKind::BlockDefinition), "back with the pick");
        let state = app.block_definition.as_ref().unwrap();
        assert_eq!(state.selected_handles.len(), 1);
    }

    #[test]
    fn a_dialog_opened_from_a_dialog_replaces_it_and_gives_it_back() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        // Plot › Edit… (plot style).
        let _ = app.update_root(Message::PlotDialogOpen);
        press_in_tools(&mut app, "Edit…");
        assert!(app.tool_open(ModalKind::Plotstyle) && !app.tool_open(ModalKind::Plot));
        let _ = app.update_root(Message::ToolWindow(
            ModalKind::Plotstyle,
            Box::new(Message::CloseModal),
        ));
        assert!(app.tool_open(ModalKind::Plot) && !app.tool_open(ModalKind::Plotstyle));
        let _ = app.update_root(Message::ToolWindow(ModalKind::Plot, Box::new(Message::CloseModal)));
        // Insert Table › Table Style….
        app.automation_op(r#"{"op":"run","cmd":"TABLE"}"#);
        let _ = app.update_root(Message::Noop);
        assert!(app.tool_open(ModalKind::InsertTable));
        press_in_tools(&mut app, "Table Style…");
        assert!(app.tool_open(ModalKind::TableStyle) && !app.tool_open(ModalKind::InsertTable));
        let _ = app.update_root(Message::ToolWindow(
            ModalKind::TableStyle,
            Box::new(Message::CloseModal),
        ));
        assert!(app.tool_open(ModalKind::InsertTable) && !app.tool_open(ModalKind::TableStyle));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_dialog_opens_in_a_window_of_its_own() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.main_window = Some(iced::window::Id::unique());
        let measured = Message::ToolWindow(
            ModalKind::About,
            Box::new(Message::ModalContentResized(Size::new(400.0, 300.0))),
        );
        // On the desktop a dialog opens in a window of its own, once its
        // content was laid out off screen, at the size it needs.
        app.active_modal = Some(ModalKind::About);
        let _ = app.update_root(Message::Noop);
        assert!(app.os_windows.is_empty(), "waits for its size");
        assert!(!app.tool_windows[0].in_canvas());
        let _ = app.update_root(measured.clone());
        let window = app.os_windows.first().copied().expect("its own window");
        assert_eq!(window.content, Detached::Tool(ModalKind::About));
        assert_eq!((window.placement.w, window.placement.h), (428.0, 328.0));
        let tool = app.tool_windows.iter().find(|t| t.kind == ModalKind::About).unwrap();
        assert!(tool.window == Some(window.id) && !tool.in_canvas());
        // A dialog's window is only the dialog: no way back into the main one.
        let view = app.os_window_view(window.id).unwrap();
        let mut ui = iced_test::simulator::Simulator::with_size(
            iced::Settings::default(),
            (428.0, 328.0),
            view,
        );
        assert!(ui.find(iced::widget::Id::new(BACK_BUTTON_ID)).is_err());
        drop(ui);
        // Esc in its window closes it, as it closed the modal.
        let _ = app.update_root(Message::OsWindowKey(window.id, "ESCAPE".into()));
        assert!(!app.tool_open(ModalKind::About));
        assert!(app.os_windows.is_empty());
        // Closed from the window manager, the dialog closes with its window.
        app.active_modal = Some(ModalKind::About);
        let _ = app.update_root(Message::Noop);
        let _ = app.update_root(measured.clone());
        let window = app.os_windows.first().copied().expect("its own window again");
        let _ = app.update_root(Message::WindowCloseRequested(window.id));
        assert!(app.os_windows.is_empty());
        assert!(!app.tool_open(ModalKind::About));
        // Moved onto a second monitor (right of a 1920 px one) and resized,
        // closed, reopened: it comes back there, at that size.
        app.active_modal = Some(ModalKind::About);
        let _ = app.update_root(Message::Noop);
        let _ = app.update_root(measured.clone());
        let window = app.os_windows[0];
        let _ = app.update_root(Message::OsWindowMoved(window.id, Point::new(2400.0, 120.0)));
        let _ = app.update_root(Message::OsWindowResized(window.id, 700.0, 500.0));
        let _ = app.update_root(Message::WindowCloseRequested(window.id));
        app.active_modal = Some(ModalKind::About);
        let _ = app.update_root(Message::Noop);
        let _ = app.update_root(measured);
        let placement = app.os_windows.first().expect("reopened").placement;
        assert_eq!(
            (placement.x, placement.y, placement.w, placement.h),
            (2400.0, 120.0, 700.0, 500.0)
        );
    }

    /// The web's "window" for a manager: it floats over the drawing; the
    /// dock's menu docks it again.
    #[test]
    fn a_manager_floats_over_the_drawing_when_called_like_a_dialog() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockMsg, PanelId};
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.dock.dock(PanelId::Layers, DockSide::Right, usize::MAX);
        app.float_pallet_centred(PanelId::Layers);
        assert!(app.dock.float_rect(PanelId::Layers).is_some());
        assert!(app.dock.location(PanelId::Layers).is_none());
        let _ = app.update_root(Message::Dock(DockMsg::EdgeMenuToggle(DockSide::Left, PanelId::Layers)));
        assert_eq!(app.dock.location(PanelId::Layers).map(|(side, _)| side), Some(DockSide::Left));
        assert!(app.dock.float_rect(PanelId::Layers).is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_manager_opens_as_a_window_or_a_pallet_by_where_it_is_called() {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockMsg, PanelId};
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.main_window = Some(iced::window::Id::unique());
        let in_window = |app: &OpenCADStudio| {
            app.os_windows
                .iter()
                .any(|window| window.content == Detached::Panel(PanelId::Layers))
        };
        // Its command: a window of its own.
        let _ = app.dispatch_command("LAYERS");
        let _ = app.update_root(Message::Noop);
        assert!(app.dock.is_separate(PanelId::Layers) && in_window(&app));
        // Closed there, then opened from the dock's pallet menu: a pallet.
        let id = app.os_windows[0].id;
        let _ = app.update_root(Message::WindowCloseRequested(id));
        assert!(!app.dock_panel_wanted(PanelId::Layers));
        let _ = app.update_root(Message::Dock(DockMsg::EdgeMenuToggle(DockSide::Right, PanelId::Layers)));
        assert!(!app.dock.is_separate(PanelId::Layers) && !in_window(&app));
        assert_eq!(app.dock.location(PanelId::Layers).map(|(side, _)| side), Some(DockSide::Right));
        assert!(app.dock_panel_wanted(PanelId::Layers));
        // Its command while it shows as a pallet leaves it there.
        let _ = app.dispatch_command("LAYERS");
        let _ = app.update_root(Message::Noop);
        assert!(!app.dock.is_separate(PanelId::Layers) && !in_window(&app));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_pallet_moves_into_its_own_window_hides_and_comes_back() {
        use crate::ui::dock::{DockMsg, PanelId};
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.main_window = Some(iced::window::Id::unique());
        let _ = app.dock_set_open(PanelId::QuickCalc, true);
        // The docked title bar's button, shown while it is hovered.
        app.dock_title_hover = Some(PanelId::QuickCalc);
        let sent = {
            let tab = &app.tabs[app.active_tab];
            press(
                app.panel_body(PanelId::QuickCalc, 320.0, false, tab),
                crate::ui::modal::DETACH_BUTTON_ID,
            )
        };
        assert!(sent
            .iter()
            .any(|m| matches!(m, Message::Dock(DockMsg::Detach(PanelId::QuickCalc)))));
        for msg in sent {
            let _ = app.update_root(msg);
        }
        assert!(app.dock.is_separate(PanelId::QuickCalc) && !app.dock.is_placed(PanelId::QuickCalc));
        let window = app.os_windows.first().copied().expect("its own window");
        assert_eq!(window.content, Detached::Panel(PanelId::QuickCalc));
        // Closed from the window manager it hides, and stays a window.
        let _ = app.update_root(Message::WindowCloseRequested(window.id));
        assert!(app.os_windows.is_empty() && !app.dock_panel_wanted(PanelId::QuickCalc));
        assert!(app.dock.is_separate(PanelId::QuickCalc));
        // Opened again (QUICKCALC), it comes back in its own window.
        let _ = app.update_root(Message::Dock(DockMsg::Close(PanelId::QuickCalc)));
        let _ = app.dock_set_open(PanelId::QuickCalc, true);
        let _ = app.update_root(Message::Noop);
        let window = app.os_windows.first().copied().expect("reopened in its own window");
        // Back into the main window, floating.
        let _ = app.update_root(Message::OsWindowReturn(window.id));
        assert!(app.os_windows.is_empty());
        assert!(!app.dock.is_separate(PanelId::QuickCalc));
        assert!(app.dock.float_rect(PanelId::QuickCalc).is_some());
    }

    #[test]
    fn plot_stays_open_beside_the_drawing() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.update_root(Message::PlotDialogOpen);
        assert_eq!(app.active_modal, None);
        assert!(app.tool_open(ModalKind::Plot));
    }

    #[test]
    fn tool_dialogs_park_beside_the_drawing_and_close_through_their_frame() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        // A tool opens the way a modal does, then parks: the drawing is free.
        app.active_modal = Some(ModalKind::About);
        let _ = app.update_root(Message::Noop);
        assert_eq!(app.active_modal, None);
        assert!(app.tool_open(ModalKind::About));
        app.active_modal = Some(ModalKind::DraftingSettings);
        let _ = app.update_root(Message::Noop);
        assert!(app.tool_open(ModalKind::About) && app.tool_open(ModalKind::DraftingSettings));
        // A message for one (its layout measuring it) keeps the stacking
        // order; opening it again brings it to the front.
        let front = |app: &OpenCADStudio| app.tool_windows.last().map(|tool| tool.kind);
        let _ = app.update_root(Message::ToolWindow(
            ModalKind::About,
            Box::new(Message::ModalContentResized(Size::new(300.0, 200.0))),
        ));
        assert_eq!(front(&app), Some(ModalKind::DraftingSettings));
        app.active_modal = Some(ModalKind::About);
        let _ = app.update_root(Message::Noop);
        assert_eq!(front(&app), Some(ModalKind::About));
        // A question parks too, and has the keyboard as it opens: Esc
        // answers it and leaves the others.
        app.active_modal = Some(ModalKind::Unsaved);
        let _ = app.update_root(Message::Noop);
        assert_eq!(app.active_modal, None);
        assert_eq!(app.keyboard_tool, Some(ModalKind::Unsaved));
        let _ = app.update_root(Message::CommandEscape);
        assert!(!app.tool_open(ModalKind::Unsaved), "Esc went to the dialog");
        assert!(app.tool_open(ModalKind::About) && app.tool_open(ModalKind::DraftingSettings));
        assert_eq!(app.keyboard_tool, None, "the keyboard is the drawing's again");
        let _ = app.update_root(Message::CommandEscape);
        assert!(app.tool_open(ModalKind::About), "Esc on the drawing leaves tools open");
        // A press on a frame takes the keyboard; one on the drawing gives it back.
        let _ = app.update_root(Message::ToolWindowRaise(ModalKind::About));
        assert_eq!(app.keyboard_tool, Some(ModalKind::About));
        let _ = app.update_root(Message::ViewportLeftPress);
        assert_eq!(app.keyboard_tool, None);
        // Its own close closes the tool, and only that one.
        let _ = app.update_root(Message::ToolWindow(
            ModalKind::About,
            Box::new(Message::CloseModal),
        ));
        assert!(!app.tool_open(ModalKind::About));
        assert!(app.tool_open(ModalKind::DraftingSettings));
        assert_eq!(app.active_modal, None);
        // Another drawing: tools on the outgoing one close.
        app.tabs
            .push(crate::app::document::DocumentTab::new_drawing(app.tabs.len() + 1));
        let other = app.tabs.len() - 1;
        let _ = app.update_root(Message::TabSwitch(other));
        assert!(!app.tool_open(ModalKind::DraftingSettings));
        // However the drawing goes (here: removed under it), its tools close.
        app.active_modal = Some(ModalKind::DraftingSettings);
        let _ = app.update_root(Message::Noop);
        assert!(app.tool_open(ModalKind::DraftingSettings));
        app.tabs.remove(other);
        app.active_tab = 0;
        let _ = app.update_root(Message::Noop);
        assert!(!app.tool_open(ModalKind::DraftingSettings));
    }
}
