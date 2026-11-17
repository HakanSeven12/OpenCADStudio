//! The enhanced attribute editor session (EATTEDIT, double-click, Properties).
//!
//! The window edits one block reference at a time, and the drawing follows
//! every change at once. Apply keeps what is shown; Cancel puts the block
//! back as it was last applied. Picking another block with Select block
//! continues the same session, and the whole session is one undo step,
//! labelled EATTEDIT, opened on the first change.
use super::{Message, ModalKind, OpenCADStudio};
use crate::modules::draw::modify::attedit::{AtteditCommand, AtteditKind};
use crate::ui::window::attribute_editor::{AttrAnswer, AttrRow, AttrTab};
use document::entities::{AttachmentPoint, HorizontalAlignment, VerticalAlignment};
use document::{EntityType, Handle};
use iced::Task;
use std::sync::{Arc, Mutex};

pub(crate) struct AttrEditorSession {
    /// The tab the edited block belongs to.
    pub tab: usize,
    /// The block reference as last applied: Cancel puts it back.
    pub applied: Arc<EntityType>,
    /// The rows as last applied: a difference is a pending change.
    pub applied_rows: Vec<AttrRow>,
    /// The undo step is open and held across messages.
    pub recording: bool,
    /// The tab's modified flag when the session began.
    pub dirty_at_open: bool,
}

/// A number as the window shows it: no trailing zeros.
pub(crate) fn number(v: f64) -> String {
    let s = format!("{:.8}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// An attribute marked annotative (its scale context data).
pub(crate) fn attribute_is_annotative(a: &document::entities::AttributeEntity) -> bool {
    a.flags.annotative
}

/// The multi-line text attachment a justification places a multi-line
/// attribute by.
pub(crate) fn attachment_for(
    h: HorizontalAlignment,
    v: VerticalAlignment,
) -> Option<AttachmentPoint> {
    use HorizontalAlignment as H;
    use VerticalAlignment as V;
    Some(match (v, h) {
        (V::Top, H::Left) => AttachmentPoint::TopLeft,
        (V::Top, H::Center) => AttachmentPoint::TopCenter,
        (V::Top, H::Right) => AttachmentPoint::TopRight,
        (V::Middle, H::Left) => AttachmentPoint::MiddleLeft,
        (V::Middle, H::Center) => AttachmentPoint::MiddleCenter,
        (V::Middle, H::Right) => AttachmentPoint::MiddleRight,
        (V::Bottom, H::Left) => AttachmentPoint::BottomLeft,
        (V::Bottom, H::Center) => AttachmentPoint::BottomCenter,
        (V::Bottom, H::Right) => AttachmentPoint::BottomRight,
        _ => return None,
    })
}

/// The justification a multi-line text attachment stands for.
pub(crate) fn alignment_of(p: AttachmentPoint) -> (HorizontalAlignment, VerticalAlignment) {
    use HorizontalAlignment as H;
    use VerticalAlignment as V;
    match p {
        AttachmentPoint::TopLeft => (H::Left, V::Top),
        AttachmentPoint::TopCenter => (H::Center, V::Top),
        AttachmentPoint::TopRight => (H::Right, V::Top),
        AttachmentPoint::MiddleLeft => (H::Left, V::Middle),
        AttachmentPoint::MiddleCenter => (H::Center, V::Middle),
        AttachmentPoint::MiddleRight => (H::Right, V::Middle),
        AttachmentPoint::BottomLeft => (H::Left, V::Bottom),
        AttachmentPoint::BottomCenter => (H::Center, V::Bottom),
        AttachmentPoint::BottomRight => (H::Right, V::Bottom),
    }
}

/// The message the reference shows for a value the window cannot keep, and
/// the tab the value is on. A zero height is not an error: it is ignored.
pub(crate) fn row_error(r: &AttrRow) -> Option<(&'static str, AttrTab)> {
    let num = |s: &str| s.trim().parse::<f64>().ok();
    const NUMERIC: &str = "Not a numeric value";
    const ANGLE: &str = "Not a valid angle value";
    const NEGATIVE: &str = "Negative values not allowed";
    match num(&r.height) {
        None => return Some((NUMERIC, AttrTab::TextOptions)),
        Some(h) if h < 0.0 => return Some((NEGATIVE, AttrTab::TextOptions)),
        _ => {}
    }
    if num(&r.rotation).is_none() {
        return Some((ANGLE, AttrTab::TextOptions));
    }
    if r.multiline {
        match num(&r.boundary_width) {
            None => return Some((NUMERIC, AttrTab::TextOptions)),
            Some(w) if w < 0.0 => return Some((NEGATIVE, AttrTab::TextOptions)),
            _ => {}
        }
        return None;
    }
    match num(&r.width_factor) {
        None => return Some((NUMERIC, AttrTab::TextOptions)),
        Some(w) if !(0.01..=100.0).contains(&w) => {
            return Some(("Value is not within range", AttrTab::TextOptions))
        }
        _ => {}
    }
    match num(&r.oblique) {
        None => Some((ANGLE, AttrTab::TextOptions)),
        Some(d) if d.abs() > 85.0 => Some((
            "Invalid Oblique angle, please enter a new angle.",
            AttrTab::TextOptions,
        )),
        _ => None,
    }
}

/// The value collected by the multi-line editor for the open window.
static EDITOR_TEXT: Mutex<Option<String>> = Mutex::new(None);

/// The value box's "…": a multi-line value is typed in the text editor, then
/// handed back to the window (`_EATTEDIT_VALUE`, or the ATTEDIT form's
/// `_ATTEDIT_FORM_VALUE`).
pub(crate) struct AttrEditorValueCommand {
    back: &'static str,
}

impl AttrEditorValueCommand {
    pub(crate) fn new(back: &'static str) -> Self {
        Self { back }
    }
}

/// The value the text editor collected, once.
pub(crate) fn take_editor_text() -> Option<String> {
    EDITOR_TEXT.lock().ok().and_then(|mut t| t.take())
}

impl crate::command::CadCommand for AttrEditorValueCommand {
    fn name(&self) -> &'static str {
        "EATTEDIT"
    }

    fn prompt(&self) -> String {
        String::new()
    }

    fn on_point(&mut self, _pt: glam::DVec3) -> crate::command::CmdResult {
        crate::command::CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> crate::command::CmdResult {
        crate::command::CmdResult::NeedPoint
    }

    fn on_editor_text(&mut self, value: String) {
        if let Ok(mut text) = EDITOR_TEXT.lock() {
            *text = Some(value);
        }
    }

    fn on_editor_closed(&mut self, _committed: bool) -> crate::command::CmdResult {
        crate::command::CmdResult::Dispatch(self.back.into())
    }
}

impl OpenCADStudio {
    /// The rows of a block reference's attributes the window lists: every
    /// attribute but the constant ones. `None` when it is not a block
    /// reference.
    fn attr_editor_rows_of(&self, i: usize, handle: Handle) -> Option<(String, Vec<AttrRow>)> {
        let doc = &self.tabs[i].scene.document;
        let Some(EntityType::Insert(ins)) = doc.get_entity(handle) else {
            return None;
        };
        let meta = crate::app::update::command::block_attr_meta(doc, &ins.block_name);
        let rows = ins
            .attributes
            .iter()
            .enumerate()
            .map(|(index, a)| {
                let mut row = crate::app::update::command::attr_row_from_entity(a, &meta);
                row.index = index;
                row
            })
            .filter(|row| !row.is_constant)
            .collect();
        Some((ins.block_name.clone(), rows))
    }

    /// Why this object cannot be edited, in the words of the prompt that
    /// picked it (`None` when it can). `Some("")` means "say nothing, ask
    /// again".
    pub(in crate::app) fn attedit_refusal(
        &self,
        i: usize,
        handle: Handle,
        kind: AtteditKind,
        reselect: bool,
    ) -> Option<&'static str> {
        let classic = kind == AtteditKind::Classic;
        if handle.is_null() {
            return Some(if classic {
                "No object found."
            } else {
                "Error selecting entity."
            });
        }
        if !matches!(
            self.tabs[i].scene.document.get_entity(handle),
            Some(EntityType::Insert(_))
        ) {
            return Some(match (classic, reselect) {
                (true, _) => "No object found.",
                (false, true) => "The selected entity is not a block.",
                (false, false) => "",
            });
        }
        if self.tabs[i].scene.locked_layer_name(handle).is_some() {
            return Some(if classic {
                "No object found."
            } else {
                "The object is on a locked layer."
            });
        }
        let editable = self
            .attr_editor_rows_of(i, handle)
            .is_some_and(|(_, rows)| !rows.is_empty());
        if !editable {
            return Some(if classic {
                "That block has no editable attributes."
            } else {
                "The selected block has no editable attributes."
            });
        }
        None
    }

    /// EATTEDIT: a single selected block reference with attributes opens at
    /// once; anything else selected is passed over (a block without
    /// editable attributes says so) and the block is asked for.
    pub(in crate::app) fn open_attedit_dialog(&mut self) {
        let i = self.active_tab;
        let selected: Vec<Handle> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        if let [handle] = selected[..] {
            let is_insert = matches!(
                self.tabs[i].scene.document.get_entity(handle),
                Some(EntityType::Insert(_))
            );
            if is_insert {
                match self.attedit_refusal(i, handle, AtteditKind::Enhanced, false) {
                    None => {
                        self.open_attribute_editor(handle);
                        return;
                    }
                    Some(msg) if !msg.is_empty() => {
                        self.command_line.push_output(crate::t!(msg).as_ref())
                    }
                    _ => {}
                }
            }
        }
        self.start_attedit_pick(AtteditKind::Enhanced, None);
    }

    /// Ask for the block: `current` is the block the window is on when
    /// Select block asks for another one.
    pub(in crate::app) fn start_attedit_pick(&mut self, kind: AtteditKind, current: Option<Handle>) {
        let i = self.active_tab;
        let cmd = AtteditCommand::new(kind, current);
        self.command_line.push_info(&crate::command::CadCommand::prompt(&cmd));
        self.tabs[i].active_cmd = Some(Box::new(cmd));
    }

    /// Open the window on a block reference, or move the open session to it.
    pub(crate) fn open_attribute_editor(&mut self, handle: Handle) {
        let i = self.active_tab;
        if let Some(msg) = self.attedit_refusal(i, handle, AtteditKind::Enhanced, false) {
            if !msg.is_empty() {
                self.command_line.push_output(crate::t!(msg).as_ref());
            }
            return;
        }
        let Some((block, rows)) = self.attr_editor_rows_of(i, handle) else {
            return;
        };
        let Some(applied) = self.tabs[i].scene.document.get_entity_arc(handle) else {
            return;
        };
        match self.attr_editor_session.as_mut() {
            Some(session) if session.tab == i => {
                session.applied = applied;
                session.applied_rows = rows.clone();
            }
            _ => {
                self.attr_editor_session = Some(AttrEditorSession {
                    tab: i,
                    applied,
                    applied_rows: rows.clone(),
                    recording: false,
                    dirty_at_open: self.tabs[i].dirty,
                });
            }
        }
        self.attr_editor_block = block;
        self.attr_editor_rows = rows;
        self.attr_editor_selected = 0;
        self.attr_editor_tab = AttrTab::Attribute;
        self.attr_editor_handle = Some(handle);
        self.attr_editor_error = None;
        self.attr_editor_asking = false;
        self.active_modal = Some(ModalKind::AttributeEditor);
        self.modal_offset = iced::Vector::ZERO;
        self.modal_resize = iced::Vector::ZERO;
    }

    /// There are changes Apply would write.
    pub(in crate::app) fn attr_editor_pending(&self) -> bool {
        self.attr_editor_session
            .as_ref()
            .is_some_and(|s| s.applied_rows != self.attr_editor_rows)
    }

    /// Show the working copy in the drawing: the block as last applied, with
    /// the rows written over it. A value the window refuses is not shown.
    pub(in crate::app) fn attr_editor_preview(&mut self) {
        let Some(handle) = self.attr_editor_handle else {
            return;
        };
        let Some(session) = self.attr_editor_session.as_mut() else {
            return;
        };
        let i = session.tab;
        if !session.recording {
            self.push_undo_snapshot(i, "EATTEDIT");
            if let Some(pending) = self.tabs[i].history.pending.as_mut() {
                pending.held = true;
            }
            if let Some(session) = self.attr_editor_session.as_mut() {
                session.recording = true;
            }
        }
        let Some(session) = self.attr_editor_session.as_ref() else {
            return;
        };
        let applied = Arc::clone(&session.applied);
        let rows = self.attr_editor_rows.clone();
        let doc = &mut self.tabs[i].scene.document;
        doc.replace_entity_arc(handle, applied);
        if let Some(EntityType::Insert(ins)) = doc.get_entity_mut(handle) {
            for row in &rows {
                if let Some(attr) = ins.attributes.get_mut(row.index) {
                    crate::app::update::command::apply_attr_row(attr, row);
                }
            }
        }
        self.tabs[i]
            .scene
            .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
        self.tabs[i].dirty = true;
    }

    /// Apply: refuse an unreadable value (the window shows why and goes to
    /// it), otherwise keep what the drawing shows.
    pub(in crate::app) fn attr_editor_apply(&mut self) -> bool {
        if let Some((idx, (msg, tab))) = self
            .attr_editor_rows
            .iter()
            .enumerate()
            .find_map(|(idx, r)| row_error(r).map(|e| (idx, e)))
        {
            self.attr_editor_error = Some(crate::t!(msg).into_owned());
            self.attr_editor_selected = idx;
            self.attr_editor_tab = tab;
            return false;
        }
        self.attr_editor_error = None;
        if !self.attr_editor_pending() {
            return true;
        }
        self.attr_editor_preview();
        let Some(handle) = self.attr_editor_handle else {
            return true;
        };
        let Some(i) = self.attr_editor_session.as_ref().map(|s| s.tab) else {
            return true;
        };
        // Fields picked in the Field dialog go onto their attributes.
        let fielded: Vec<(usize, Option<(String, Vec<Handle>)>, String)> = self
            .attr_editor_rows
            .iter()
            .filter(|r| r.field_changed)
            .map(|r| (r.index, r.field.clone(), r.value.clone()))
            .collect();
        for (index, field, value) in fielded {
            let attr_handle = match self.tabs[i].scene.document.get_entity(handle) {
                Some(EntityType::Insert(ins)) => ins.attributes.get(index).map(|a| a.common.handle),
                _ => None,
            };
            if let Some(attr_handle) = attr_handle.filter(|h| !h.is_null()) {
                crate::entities::field::set_text_field(
                    &mut self.tabs[i].scene.document,
                    attr_handle,
                    field,
                    &value,
                );
            }
        }
        for row in &mut self.attr_editor_rows {
            row.field_changed = false;
        }
        let applied = self.tabs[i].scene.document.get_entity_arc(handle);
        if let (Some(session), Some(applied)) = (self.attr_editor_session.as_mut(), applied) {
            session.applied = applied;
            session.applied_rows = self.attr_editor_rows.clone();
        }
        self.refresh_properties();
        true
    }

    /// End the session: OK keeps what was applied, Cancel (or ✕) puts the
    /// block back as it was last applied. Either way the session's changes
    /// are one undo step.
    pub(in crate::app) fn attr_editor_end(&mut self, keep: bool) {
        if let Some(session) = self.attr_editor_session.take() {
            let i = session.tab;
            if !keep {
                if let Some(handle) = self.attr_editor_handle {
                    if i < self.tabs.len() {
                        self.tabs[i]
                            .scene
                            .document
                            .replace_entity_arc(handle, Arc::clone(&session.applied));
                        self.tabs[i]
                            .scene
                            .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                    }
                }
            }
            if session.recording && i < self.tabs.len() {
                if let Some(pending) = self.tabs[i].history.pending.as_mut() {
                    pending.held = false;
                }
                let unchanged = self.tabs[i].history.pending.as_ref().is_some_and(|_| {
                    self.attr_editor_handle.is_none_or(|h| {
                        let now = self.tabs[i].scene.document.get_entity_arc(h);
                        now.is_some_and(|now| Arc::ptr_eq(&now, &session.applied))
                    })
                });
                if unchanged && !keep {
                    self.tabs[i].dirty = session.dirty_at_open;
                }
                self.finish_pending_history(i);
            }
        }
        if self.focus_dialog(ModalKind::AttributeEditor) {
            self.active_modal = None;
            self.reset_modal_geometry();
        }
        self.attr_editor_handle = None;
        self.attr_editor_block.clear();
        self.attr_editor_rows.clear();
        self.attr_editor_selected = 0;
        self.attr_editor_tab = AttrTab::Attribute;
        self.attr_editor_error = None;
        self.attr_editor_asking = false;
        self.refresh_properties();
    }

    /// The tab or the drawing the window works on goes away: what was not
    /// applied is taken back.
    pub(in crate::app) fn cancel_attr_editor(&mut self) {
        if self.attr_editor_session.is_some() || self.attr_editor_handle.is_some() {
            self.attr_editor_end(false);
        }
    }

    /// Select block: with changes pending the window asks first.
    fn attr_editor_select_block(&mut self, answer: Option<AttrAnswer>) {
        match answer {
            None if self.attr_editor_pending() => {
                self.attr_editor_asking = true;
                return;
            }
            Some(AttrAnswer::Stay) => {
                self.attr_editor_asking = false;
                return;
            }
            Some(AttrAnswer::Save) => {
                self.attr_editor_asking = false;
                if !self.attr_editor_apply() {
                    return;
                }
            }
            Some(AttrAnswer::Discard) => {
                self.attr_editor_asking = false;
                if let Some(rows) = self.attr_editor_session.as_ref().map(|s| s.applied_rows.clone()) {
                    self.attr_editor_rows = rows;
                    self.attr_editor_preview();
                }
            }
            None => {}
        }
        // The window steps aside while the block is picked; the session
        // stays.
        self.active_modal = None;
        let current = self.attr_editor_handle;
        self.start_attedit_pick(AtteditKind::Enhanced, current);
    }

    /// `_EATTEDIT_VALUE`: the multi-line editor closed.
    pub(in crate::app) fn attr_editor_value_back(&mut self) {
        if let Some(value) = take_editor_text() {
            if let Some(r) = self.attr_editor_rows.get_mut(self.attr_editor_selected) {
                r.value = value;
                r.field = None;
                r.field_changed = true;
            }
            self.attr_editor_preview();
        }
        if self.attr_editor_session.is_some() {
            self.active_modal = Some(ModalKind::AttributeEditor);
        }
    }

    /// The Field dialog closed with a field for the selected row.
    pub(in crate::app) fn attr_editor_field_back(
        &mut self,
        field: Option<(String, Vec<Handle>)>,
        value: String,
    ) {
        if let Some(r) = self.attr_editor_rows.get_mut(self.attr_editor_selected) {
            r.value = value;
            r.field = field;
            r.field_changed = true;
        }
        self.attr_editor_preview();
        self.active_modal = Some(ModalKind::AttributeEditor);
    }

    /// The window's own messages (the field edits are in `update`).
    pub(in crate::app) fn on_attr_editor_msg(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AttrEditorApply => {
                self.attr_editor_apply();
            }
            Message::AttrEditorOk => {
                if self.attr_editor_apply() {
                    self.attr_editor_end(true);
                }
            }
            Message::AttrEditorCancel => self.attr_editor_end(false),
            Message::AttrEditorHelp => self.command_line.push_info(
                crate::t!(
                    "Edits the values, text options and properties of the attributes of a block reference."
                )
                .as_ref(),
            ),
            Message::AttrEditorSelectBlock => self.attr_editor_select_block(None),
            Message::AttrEditorAnswer(answer) => self.attr_editor_select_block(Some(answer)),
            Message::AttrEditorDismissError => self.attr_editor_error = None,
            Message::AttrEditorEditValue => {
                let i = self.active_tab;
                let Some(row) = self.attr_editor_rows.get(self.attr_editor_selected) else {
                    return Task::none();
                };
                let initial = row.value.clone();
                let height = row.height.trim().parse::<f64>().ok().filter(|h| *h > 0.0).unwrap_or(2.5);
                let pos = self
                    .attr_editor_handle
                    .and_then(|h| match self.tabs[i].scene.document.get_entity(h) {
                        Some(EntityType::Insert(ins)) => ins.attributes.get(row.index).map(|a| {
                            glam::DVec3::new(a.insertion_point.x, a.insertion_point.y, a.insertion_point.z)
                        }),
                        _ => None,
                    })
                    .unwrap_or_else(|| self.tabs[i].scene.camera.borrow().target);
                self.active_modal = None;
                if let Ok(mut text) = EDITOR_TEXT.lock() {
                    *text = None;
                }
                self.tabs[i].active_cmd =
                    Some(Box::new(AttrEditorValueCommand::new("_EATTEDIT_VALUE")));
                return self.apply_cmd_result(crate::command::CmdResult::SuspendForMTextInput {
                    pos,
                    initial,
                    height,
                });
            }
            Message::AttrEditorInsertField => {
                self.active_modal = None;
                self.open_field_dialog(crate::ui::window::field_dialog::FieldTarget::AttrEditor);
            }
            Message::AttrEditorBoundary(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.boundary_width = s;
                }
                self.attr_editor_preview();
            }
            Message::AttrEditorColorValue(color) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.color = color;
                }
                self.attr_editor_preview();
            }
            _ => {}
        }
        Task::none()
    }
}
