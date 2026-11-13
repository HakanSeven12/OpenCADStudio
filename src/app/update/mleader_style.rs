//! The multileader style manager: list, New, Modify and their choosers.
//!
//! The window is one transaction: Set Current, a new style, Delete and
//! Modify's OK change the drawing live, and closing the window keeps them as
//! one undo step. Cancel in Modify (or its ✕) puts the style back as it was
//! when Modify opened; a style made by New is dropped again.

use crate::app::{Message, ModalKind, OpenCADStudio, StyleKind};
use crate::ui::style::mleaderstyle::{
    field_id, field_tab, trim_number, CreateDraft, LoadDraft, MlsMsg, Modify, Notice, Pick,
    ReloadAsk, OTHER,
};
use document::objects::{
    BlockContentConnectionType, LeaderContentType, MultiLeaderPathType, MultiLeaderStyle,
    ObjectType, TextAngleType, TextAttachmentDirectionType, TextAttachmentType,
};
use document::Handle;
use iced::Task;
use std::sync::Mutex;

/// The default text collected by the multi-line editor for the open Modify.
static EDITOR_TEXT: Mutex<Option<String>> = Mutex::new(None);

/// Modify's Default text "…": the value is typed in the multi-line text
/// editor, then handed back to the window (`_MLEADERSTYLE_TEXT`).
pub(crate) struct MleaderStyleTextCommand;

impl crate::command::CadCommand for MleaderStyleTextCommand {
    fn name(&self) -> &'static str {
        "MLEADERSTYLE"
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
        crate::command::CmdResult::Dispatch("_MLEADERSTYLE_TEXT".into())
    }
}

/// A typed value a field accepts: a number, not negative.
fn non_negative(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

fn attachment(value: &str) -> TextAttachmentType {
    match value {
        "TopOfTopLine" => TextAttachmentType::TopOfTopLine,
        "MiddleOfText" => TextAttachmentType::MiddleOfText,
        "MiddleOfBottomLine" => TextAttachmentType::MiddleOfBottomLine,
        "BottomOfBottomLine" => TextAttachmentType::BottomOfBottomLine,
        "BottomLine" => TextAttachmentType::BottomLine,
        "BottomOfTopLineUnderlineBottomLine" => {
            TextAttachmentType::BottomOfTopLineUnderlineBottomLine
        }
        "BottomOfTopLineUnderlineTopLine" => TextAttachmentType::BottomOfTopLineUnderlineTopLine,
        "BottomOfTopLineUnderlineAll" => TextAttachmentType::BottomOfTopLineUnderlineAll,
        "CenterOfText" => TextAttachmentType::CenterOfText,
        "CenterOfTextOverline" => TextAttachmentType::CenterOfTextOverline,
        _ => TextAttachmentType::MiddleOfTopLine,
    }
}

/// The characters a style name may not hold.
fn unsupported_name(name: &str) -> bool {
    name.trim().is_empty()
        || name.chars().any(|c| {
            matches!(
                c,
                '\\' | '<' | '>' | '/' | '?' | '"' | ':' | ';' | '*' | '|' | ',' | '=' | '`'
            )
        })
}

/// The drawing's blocks a user arrow or content block can be: no layouts,
/// anonymous or referenced blocks, nor the predefined arrow and content
/// blocks.
pub(crate) fn user_blocks(doc: &document::Document) -> Vec<String> {
    let mut names: Vec<String> = doc
        .block_records
        .iter()
        .filter(|b| {
            !b.is_layout()
                && !b.is_model_space()
                && !b.is_paper_space()
                && !b.name.starts_with('*')
                && !b.flags.is_xref
                && !b.flags.is_xref_overlay
                && !crate::entities::arrowheads::STANDARD
                    .iter()
                    .any(|(_, block)| !block.is_empty() && block.eq_ignore_ascii_case(&b.name))
                && !crate::entities::leader_blocks::is_standard_block(&b.name)
        })
        .map(|b| b.name.clone())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names
}

impl OpenCADStudio {
    /// Mutable access to the currently selected multileader style.
    pub(in crate::app) fn mleaderstyle_mut(&mut self, tab: usize) -> Option<&mut MultiLeaderStyle> {
        let name = self.mleaderstyle_selected.clone();
        self.tabs[tab]
            .scene
            .document
            .objects
            .values_mut()
            .find_map(|o| match o {
                ObjectType::MultiLeaderStyle(s) if s.name == name => Some(s),
                _ => None,
            })
    }

    fn mleaderstyle_ref(&self, tab: usize) -> Option<&MultiLeaderStyle> {
        let name = &self.mleaderstyle_selected;
        self.tabs[tab]
            .scene
            .document
            .objects
            .values()
            .find_map(|o| match o {
                ObjectType::MultiLeaderStyle(s) if &s.name == name => Some(s),
                _ => None,
            })
    }

    /// Fill the editor's typed values from the selected multileader style.
    pub(in crate::app) fn load_mleaderstyle_bufs(&mut self, tab: usize) {
        let Some(s) = self.mleaderstyle_ref(tab).cloned() else {
            return;
        };
        let color = |c: document::types::Color| c.index().map(|c| c.to_string()).unwrap_or_default();
        self.mls_landing_distance = trim_number(s.landing_distance);
        self.mls_landing_gap = trim_number(s.landing_gap);
        self.mls_arrowhead_size = trim_number(s.arrowhead_size);
        self.mls_text_height = trim_number(s.text_height);
        // Scale to layout stores 0; Specify scale comes back at 1.
        self.mls_scale_factor = if s.scale_factor == 0.0 {
            "1".to_string()
        } else {
            trim_number(s.scale_factor)
        };
        self.mls_break_gap = trim_number(s.break_gap_size);
        self.mls_max_points = if s.max_leader_points > 0 {
            s.max_leader_points.to_string()
        } else {
            "2".to_string()
        };
        self.mls_default_text = s.default_text.clone();
        self.mls_line_color = color(s.line_color);
        self.mls_text_color = color(s.text_color);
        self.mls_block_color = color(s.block_content_color);
        self.mls_block_scale = trim_number(s.block_content_scale_x);
    }

    pub(super) fn on_mleader_style_dialog_open(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let cur = self.tabs[i].active_mleader_style.clone();
        let names: Vec<String> = self.tabs[i]
            .scene
            .document
            .objects
            .values()
            .filter_map(|o| match o {
                ObjectType::MultiLeaderStyle(s) => Some(s.name.clone()),
                _ => None,
            })
            .collect();
        self.mleaderstyle_selected = names
            .iter()
            .find(|n| n.eq_ignore_ascii_case(&cur))
            .or_else(|| names.iter().min_by_key(|n| n.to_lowercase()))
            .cloned()
            .unwrap_or_else(|| "Standard".to_string());
        self.mls = Default::default();
        self.mls_color_open = None;
        self.load_mleaderstyle_bufs(i);
        self.active_modal = Some(ModalKind::MLeaderStyle);
        // Linetypes loaded from Select Linetype belong to the session too.
        self.style_stage_begin_linetypes();
        Task::none()
    }

    fn mleader_style_set_current(&mut self) {
        let i = self.active_tab;
        let name = self.mleaderstyle_selected.clone();
        if self.mleaderstyle_ref(i).is_none() {
            return;
        }
        // The header field is the round-trip source of truth
        // ($CMLEADERSTYLE); the ribbon and tab mirror it.
        self.tabs[i]
            .scene
            .document
            .header
            .current_mleader_style_name = name.clone();
        self.tabs[i].active_mleader_style = name.clone();
        self.ribbon.active_mleader_style = name;
    }

    pub(super) fn on_mleader_style_edit(&mut self, field: &'static str, value: String) -> Task<Message> {
        self.mls_color_open = None;
        match field {
            "landing_distance" => self.mls_landing_distance = value,
            "landing_gap" => self.mls_landing_gap = value,
            "arrowhead_size" => self.mls_arrowhead_size = value,
            "text_height" => self.mls_text_height = value,
            "scale_factor" => self.mls_scale_factor = value,
            "break_gap" => self.mls_break_gap = value,
            "max_points" => self.mls_max_points = value,
            "default_text" => self.mls_default_text = value,
            "line_color" => self.mls_line_color = value,
            "text_color" => self.mls_text_color = value,
            "block_color" => self.mls_block_color = value,
            "block_scale" => self.mls_block_scale = value,
            _ => {}
        }
        Task::none()
    }

    pub(super) fn on_mleader_style_toggle(&mut self, field: &'static str) -> Task<Message> {
        let i = self.active_tab;
        if let Some(s) = self.mleaderstyle_mut(i) {
            match field {
                "enable_landing" => s.enable_landing = !s.enable_landing,
                "enable_dogleg" => s.enable_dogleg = !s.enable_dogleg,
                "text_frame" => s.text_frame = !s.text_frame,
                "text_always_left" => s.text_always_left = !s.text_always_left,
                "annotative" => s.is_annotative = !s.is_annotative,
                "extend_leader" => s.unknown_flag_298 = !s.unknown_flag_298,
                _ => {}
            }
        }
        Task::none()
    }

    pub(super) fn on_mleader_style_set_enum(&mut self, field: &'static str, value: String) -> Task<Message> {
        let i = self.active_tab;
        if let Some(s) = self.mleaderstyle_mut(i) {
            match field {
                "path_type" => {
                    s.path_type = match value.as_str() {
                        "Invisible" => MultiLeaderPathType::Invisible,
                        "Spline" => MultiLeaderPathType::Spline,
                        _ => MultiLeaderPathType::StraightLineSegments,
                    };
                }
                "content_type" => {
                    s.content_type = match value.as_str() {
                        "None" => LeaderContentType::None,
                        "Block" => LeaderContentType::Block,
                        _ => LeaderContentType::MText,
                    };
                }
                "text_angle_type" => {
                    s.text_angle_type = match value.as_str() {
                        "ParallelToLastLeaderLine" => TextAngleType::ParallelToLastLeaderLine,
                        "Optimized" => TextAngleType::Optimized,
                        _ => TextAngleType::Horizontal,
                    };
                }
                "text_left_attachment" => s.text_left_attachment = attachment(&value),
                "text_right_attachment" => s.text_right_attachment = attachment(&value),
                "text_top_attachment" => s.text_top_attachment = attachment(&value),
                "text_bottom_attachment" => s.text_bottom_attachment = attachment(&value),
                "text_attachment_direction" => {
                    s.text_attachment_direction = match value.as_str() {
                        "Vertical" => TextAttachmentDirectionType::Vertical,
                        _ => TextAttachmentDirectionType::Horizontal,
                    };
                }
                "block_content_connection" => {
                    s.block_content_connection = match value.as_str() {
                        "BasePoint" => BlockContentConnectionType::BasePoint,
                        _ => BlockContentConnectionType::BlockExtents,
                    };
                }
                "first_seg_angle" | "second_seg_angle" => {
                    if let Ok(degrees) = value.trim().parse::<f64>() {
                        if field == "first_seg_angle" {
                            s.first_segment_angle = degrees.to_radians();
                        } else {
                            s.second_segment_angle = degrees.to_radians();
                        }
                    }
                }
                _ => {}
            }
        }
        Task::none()
    }

    /// The line type, arrowhead, text style or content block a list picked;
    /// a list's last entry opens its chooser instead.
    pub(super) fn on_mleader_style_set_handle(&mut self, field: &'static str, value: String) -> Task<Message> {
        let i = self.active_tab;
        self.mls_color_open = None;
        if value == OTHER {
            let doc = &self.tabs[i].scene.document;
            let style = self.mleaderstyle_ref(i);
            self.mls.pick = Some(match field {
                "line_type_handle" => Pick::Linetype(
                    style
                        .and_then(|s| s.line_type_handle)
                        .and_then(|h| doc.line_types.iter().find(|lt| lt.handle == h))
                        .map(|lt| lt.name.clone())
                        .unwrap_or_else(|| "ByBlock".to_string()),
                ),
                "arrowhead_handle" => Pick::UserArrow(String::new()),
                _ => Pick::UserBlock(String::new()),
            });
            return Task::none();
        }
        let doc = &mut self.tabs[i].scene.document;
        let handle: Option<Handle> = match field {
            "arrowhead_handle" => crate::entities::arrowheads::resolve(doc, &value).filter(|h| !h.is_null()),
            "block_content_handle" => crate::entities::leader_blocks::resolve(doc, &value),
            "line_type_handle" => doc
                .line_types
                .iter()
                .find(|lt| lt.name.eq_ignore_ascii_case(&value))
                .map(|lt| lt.handle),
            "text_style_handle" => doc
                .text_styles
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(&value))
                .map(|t| t.handle),
            _ => None,
        };
        if let Some(s) = self.mleaderstyle_mut(i) {
            match field {
                "line_type_handle" => s.line_type_handle = handle,
                "text_style_handle" => s.text_style_handle = handle,
                "arrowhead_handle" => s.arrowhead_handle = handle,
                "block_content_handle" => s.block_content_handle = handle,
                _ => {}
            }
        }
        Task::none()
    }

    /// Check every typed value, then write them into the style. A value the
    /// style cannot take is refused (the field that holds it), as OK refuses
    /// it: a word, or a negative size.
    fn mleader_style_stage_bufs(&mut self) -> Result<(), &'static str> {
        let i = self.active_tab;
        let Some(m) = self.mls.modify.clone() else {
            return Ok(());
        };
        let annotative = self.mleaderstyle_ref(i).is_some_and(|s| s.is_annotative);
        let number = |value: &str, field: &'static str| non_negative(value).ok_or(field);
        let arrow = number(&self.mls_arrowhead_size, "arrowhead_size")?;
        let gap = number(&self.mls_break_gap, "break_gap")?;
        let max_points = if m.max_points_on {
            // A whole number; fewer than two points is two.
            let points = self
                .mls_max_points
                .trim()
                .parse::<i32>()
                .map_err(|_| "max_points")?;
            points.max(2)
        } else {
            0
        };
        let landing = number(&self.mls_landing_distance, "landing_distance")?;
        let scale = if annotative || m.scale_to_layout {
            None
        } else {
            Some(number(&self.mls_scale_factor, "scale_factor")?)
        };
        let text_height = number(&self.mls_text_height, "text_height")?;
        let landing_gap = number(&self.mls_landing_gap, "landing_gap")?;
        let block_scale = number(&self.mls_block_scale, "block_scale")?;
        let color = |value: &str| value.parse::<i16>().ok().map(document::types::Color::from_index);
        let (line_color, text_color, block_color) = (
            color(&self.mls_line_color),
            color(&self.mls_text_color),
            color(&self.mls_block_color),
        );
        let default_text = self.mls_default_text.clone();
        let Some(s) = self.mleaderstyle_mut(i) else {
            return Ok(());
        };
        s.arrowhead_size = arrow;
        s.break_gap_size = gap;
        s.max_leader_points = max_points;
        if !m.first_angle_on {
            s.first_segment_angle = 0.0;
        }
        if !m.second_angle_on {
            s.second_segment_angle = 0.0;
        }
        s.landing_distance = landing;
        if m.scale_to_layout && !annotative {
            s.scale_factor = 0.0;
        } else if let Some(scale) = scale {
            s.scale_factor = scale;
        }
        s.text_height = text_height;
        s.landing_gap = landing_gap;
        s.set_block_scale(block_scale);
        if let Some(c) = line_color {
            s.line_color = c;
        }
        if let Some(c) = text_color {
            s.text_color = c;
        }
        if let Some(c) = block_color {
            s.block_content_color = c;
        }
        s.default_text = default_text;
        Ok(())
    }

    /// Modify's Cancel: the style as it was when Modify opened, or a style
    /// New made taken out again.
    fn mleader_style_cancel_modify(&mut self) {
        let Some(m) = self.mls.modify.take() else {
            return;
        };
        self.mls.pick = None;
        self.mls_color_open = None;
        let i = self.active_tab;
        let doc = &mut self.tabs[i].scene.document;
        match m.backup {
            Some(style) => {
                doc.objects.insert(m.handle, ObjectType::MultiLeaderStyle(style));
            }
            None => {
                crate::app::style_ops::remove_object(doc, m.handle);
                self.mleaderstyle_selected = m.previous_selected;
            }
        }
        self.after_style_change(StyleKind::MLeader);
        self.load_mleaderstyle_bufs(i);
    }

    /// The ✕ of the window closes its topmost face first; `false` when only
    /// the manager is open.
    pub(super) fn mleader_style_close_face(&mut self) -> bool {
        if self.mls.reload_ask.take().is_some() {
            self.mls.load = None;
            return true;
        }
        if self.mls.notice.take().is_some()
            || self.mls.delete_ask.take().is_some()
            || self.mls.load.take().is_some()
            || self.mls.pick.take().is_some()
            || self.mls.create.take().is_some()
        {
            return true;
        }
        if self.mls.modify.is_some() {
            self.mleader_style_cancel_modify();
            return true;
        }
        false
    }

    /// The window closes: an open Modify is cancelled and the session's
    /// changes become one undo step.
    pub(super) fn mleader_style_close(&mut self) {
        self.mleader_style_cancel_modify();
        self.mls = Default::default();
        self.mls_color_open = None;
        self.after_style_change(StyleKind::MLeader);
        self.style_stage_commit();
        self.style_stage = None;
    }

    /// The Text Style manager opened from Modify closed: what it did not
    /// apply is taken back and Modify comes back in the same transaction.
    pub(super) fn mleader_style_back_from_text_styles(&mut self) {
        self.style_stage_child_close();
        self.active_modal = Some(ModalKind::MLeaderStyle);
        self.reset_modal_geometry();
    }

    /// `_MLEADERSTYLE_TEXT`: the multi-line editor closed.
    pub(in crate::app) fn mleader_style_text_back(&mut self) -> Task<Message> {
        if let Some(value) = EDITOR_TEXT.lock().ok().and_then(|mut t| t.take()) {
            self.mls_default_text = value;
        }
        if self.mls.modify.is_some() {
            self.active_modal = Some(ModalKind::MLeaderStyle);
        }
        Task::none()
    }

    fn mleader_style_names(&self) -> Vec<String> {
        self.style_names(StyleKind::MLeader)
    }

    pub(super) fn on_mleader_style_dlg(&mut self, message: MlsMsg) -> Task<Message> {
        let i = self.active_tab;
        match message {
            MlsMsg::Select(name) => {
                if self.mls.modify.is_none() {
                    self.mleaderstyle_selected = name;
                    self.load_mleaderstyle_bufs(i);
                }
            }
            MlsMsg::ListFilter(in_use) => self.mls.in_use_only = in_use,
            MlsMsg::SetCurrent => self.mleader_style_set_current(),
            MlsMsg::New => {
                let start = self.mleaderstyle_selected.clone();
                let annotative = self.mleaderstyle_ref(i).is_some_and(|s| s.is_annotative);
                self.mls.create = Some(CreateDraft {
                    name: crate::i18n::plain(crate::tr!("mleader-style", "copy-of", name = start.clone())),
                    start_with: start,
                    annotative,
                    name_edited: false,
                });
                let id = iced::widget::Id::new("mls-new-name");
                return Task::batch([
                    iced::widget::operation::focus(id.clone()),
                    iced::widget::operation::select_all(id),
                ]);
            }
            MlsMsg::CreateName(name) => {
                if let Some(draft) = self.mls.create.as_mut() {
                    draft.name = name;
                    draft.name_edited = true;
                }
            }
            MlsMsg::CreateStart(start) => {
                let annotative = self.tabs[i].scene.document.objects.values().any(|o| {
                    matches!(o, ObjectType::MultiLeaderStyle(s)
                        if s.name.eq_ignore_ascii_case(&start) && s.is_annotative)
                });
                if let Some(draft) = self.mls.create.as_mut() {
                    // Until the name is typed, the copy follows the style it
                    // starts from: its name and its annotative setting.
                    if !draft.name_edited {
                        draft.name = crate::i18n::plain(crate::tr!("mleader-style", "copy-of", name = start.clone()));
                        draft.annotative = annotative;
                    }
                    draft.start_with = start;
                }
            }
            MlsMsg::CreateAnnotative(on) => {
                if let Some(draft) = self.mls.create.as_mut() {
                    draft.annotative = on;
                }
            }
            MlsMsg::CreateCancel => self.mls.create = None,
            MlsMsg::CreateContinue => return self.mleader_style_create(),
            MlsMsg::NoticeClose => {
                self.mls.notice = None;
                if self.mls.create.is_some() {
                    let id = iced::widget::Id::new("mls-new-name");
                    return Task::batch([
                        iced::widget::operation::focus(id.clone()),
                        iced::widget::operation::select_all(id),
                    ]);
                }
            }
            MlsMsg::Modify => {
                if let Some(style) = self.mleaderstyle_ref(i).cloned() {
                    self.mls.modify = Some(Modify {
                        handle: style.handle,
                        previous_selected: style.name.clone(),
                        tab: 0,
                        max_points_on: style.max_leader_points > 0,
                        first_angle_on: style.first_segment_angle != 0.0,
                        second_angle_on: style.second_segment_angle != 0.0,
                        scale_to_layout: style.scale_factor == 0.0 && !style.is_annotative,
                        backup: Some(style),
                    });
                    self.load_mleaderstyle_bufs(i);
                }
            }
            MlsMsg::Delete => {
                let name = self.mleaderstyle_selected.clone();
                let doc = &self.tabs[i].scene.document;
                let current = &self.tabs[i].active_mleader_style;
                let used = crate::app::style_ops::style_in_use(doc, StyleKind::MLeader, &name, current);
                if !name.eq_ignore_ascii_case("Standard") && !used {
                    self.mls.delete_ask = Some(name);
                }
            }
            MlsMsg::DeleteAnswer(yes) => {
                if let Some(name) = self.mls.delete_ask.take() {
                    if yes && self.remove_style_storage(StyleKind::MLeader, &name) {
                        let mut names = self.mleader_style_names();
                        names.sort_by_key(|n| n.to_lowercase());
                        self.mleaderstyle_selected = names
                            .into_iter()
                            .next()
                            .unwrap_or_else(|| "Standard".to_string());
                        self.after_style_change(StyleKind::MLeader);
                        self.load_mleaderstyle_bufs(i);
                    }
                }
            }
            MlsMsg::Help => {
                let help = if self.mls.modify.is_some() {
                    crate::tr!("mleader-style", "help-modify")
                } else if self.mls.create.is_some() {
                    crate::tr!("mleader-style", "help-create")
                } else {
                    crate::tr!("mleader-style", "help-manager")
                };
                self.command_line.push_info(&help);
            }
            MlsMsg::Tab(tab) => {
                self.mls_color_open = None;
                if let Some(m) = self.mls.modify.as_mut() {
                    m.tab = tab;
                }
            }
            MlsMsg::Constraint(key, on) => {
                if let Some(m) = self.mls.modify.as_mut() {
                    match key {
                        "max_points" => m.max_points_on = on,
                        "first_angle" => m.first_angle_on = on,
                        _ => m.second_angle_on = on,
                    }
                }
            }
            MlsMsg::ScaleToLayout(on) => {
                if let Some(m) = self.mls.modify.as_mut() {
                    // Back to a specified scale it starts again from 1.
                    if m.scale_to_layout && !on {
                        self.mls_scale_factor = "1".to_string();
                    }
                    m.scale_to_layout = on;
                }
            }
            MlsMsg::PickChoice(value) => match self.mls.pick.as_mut() {
                Some(Pick::Linetype(name) | Pick::UserArrow(name) | Pick::UserBlock(name)) => {
                    *name = value;
                }
                None => {}
            },
            // Load… lists the program's linetypes first; File… picks a
            // `.lin` file instead.
            MlsMsg::PickLoad => {
                let source = crate::io::linetypes::catalog_source();
                self.mls.load = Some(LoadDraft {
                    file: String::new(),
                    dir: None,
                    entries: crate::io::linetypes::source_entries(&source),
                    source,
                    picked: Vec::new(),
                });
            }
            MlsMsg::LoadFile => {
                return Task::perform(crate::io::pick_linetype_file_named(), |picked| {
                    Message::MLeaderStyleDlg(MlsMsg::LoadFileChosen(picked))
                });
            }
            MlsMsg::LoadFileChosen(Some(file)) => {
                if let Some(load) = self.mls.load.as_mut() {
                    load.entries = crate::io::linetypes::source_entries(&file.source);
                    load.file = file.name;
                    load.source = file.source;
                    load.dir = file.dir;
                    load.picked.clear();
                }
            }
            MlsMsg::LoadFileChosen(None) => {}
            MlsMsg::LoadToggle(name) => {
                if let Some(load) = self.mls.load.as_mut() {
                    if let Some(at) = load.picked.iter().position(|p| p.eq_ignore_ascii_case(&name)) {
                        load.picked.remove(at);
                    } else {
                        load.picked.push(name);
                    }
                }
            }
            MlsMsg::LoadCancel => self.mls.load = None,
            // OK asks about the ticked linetypes the drawing already has,
            // one by one, before loading anything.
            MlsMsg::LoadOk => {
                if let Some(load) = self.mls.load.as_ref() {
                    match ReloadAsk::for_load(load, &self.tabs[i].scene.document) {
                        Some(ask) => self.mls.reload_ask = Some(ask),
                        None => self.mleader_style_finish_load(Vec::new()),
                    }
                }
            }
            MlsMsg::ReloadAnswer(answer) => {
                let Some(mut ask) = self.mls.reload_ask.take() else {
                    return Task::none();
                };
                match ask.answer(answer) {
                    Some(Ok(reload)) => self.mleader_style_finish_load(reload),
                    // Nothing is loaded and Load Linetypes closes.
                    Some(Err(())) => self.mls.load = None,
                    None => self.mls.reload_ask = Some(ask),
                }
            }
            MlsMsg::PickCancel => self.mls.pick = None,
            MlsMsg::PickOk => {
                let field = match self.mls.pick.take() {
                    Some(Pick::Linetype(name)) => Some(("line_type_handle", name)),
                    Some(Pick::UserArrow(name)) => Some(("arrowhead_handle", name)),
                    Some(Pick::UserBlock(name)) => Some(("block_content_handle", name)),
                    None => None,
                };
                if let Some((field, name)) = field.filter(|(_, name)| !name.is_empty()) {
                    return self.on_mleader_style_set_handle(field, name);
                }
            }
            MlsMsg::DefaultText => {
                // The window waits while the multi-line editor collects the text.
                let initial = self.mls_default_text.clone();
                let height = non_negative(&self.mls_text_height)
                    .filter(|h| *h > 0.0)
                    .unwrap_or(2.5);
                self.mls_color_open = None;
                self.active_modal = None;
                if let Ok(mut text) = EDITOR_TEXT.lock() {
                    *text = None;
                }
                self.tabs[i].active_cmd = Some(Box::new(MleaderStyleTextCommand));
                let pos = self.tabs[i].scene.camera.borrow().target;
                return self.apply_cmd_result(crate::command::CmdResult::SuspendForMTextInput {
                    pos,
                    initial,
                    height,
                });
            }
            MlsMsg::TextStyles => return self.mleader_style_open_text_styles(),
            MlsMsg::Ok => return self.mleader_style_ok(),
            MlsMsg::Cancel => self.mleader_style_cancel_modify(),
        }
        Task::none()
    }

    /// Load Linetypes ends: the ticked linetypes the drawing lacks are
    /// added, `reload` are read again; Select Linetype keeps its choice.
    fn mleader_style_finish_load(&mut self, reload: Vec<String>) {
        let Some(load) = self.mls.load.take() else {
            return;
        };
        let doc = &mut self.tabs[self.active_tab].scene.document;
        crate::io::linetypes::populate_document_named(
            doc,
            &load.source,
            load.dir.as_deref(),
            &load.picked,
        );
        let reloaded = crate::io::linetypes::reload_document_named(
            doc,
            &load.source,
            load.dir.as_deref(),
            &reload,
        );
        // Entities drawn with a reloaded linetype take its new pattern.
        if reloaded > 0 {
            self.tabs[self.active_tab]
                .scene
                .invalidate_linetype_dependencies(&reload);
        }
    }

    fn mleader_style_create(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some(draft) = self.mls.create.clone() else {
            return Task::none();
        };
        let notice = if draft.name.is_empty() {
            Some(Notice::EmptyName)
        } else if unsupported_name(&draft.name) {
            Some(Notice::UnsupportedCharacters)
        } else if self.style_exists(StyleKind::MLeader, &draft.name) {
            Some(Notice::Exists(draft.name.clone()))
        } else {
            None
        };
        if notice.is_some() {
            self.mls.notice = notice;
            return Task::none();
        }
        let handle = self.tabs[i].scene.document.allocate_handle();
        if !self.clone_style_as(StyleKind::MLeader, &draft.start_with, &draft.name, handle) {
            self.insert_mleader_style_default(&draft.name, handle);
        }
        if let Some(ObjectType::MultiLeaderStyle(style)) =
            self.tabs[i].scene.document.objects.get_mut(&handle)
        {
            style.is_annotative = draft.annotative;
        }
        let previous = self.mleaderstyle_selected.clone();
        self.mleaderstyle_selected = draft.name.clone();
        self.after_style_change(StyleKind::MLeader);
        self.mls.create = None;
        let style = self.mleaderstyle_ref(i).cloned();
        if let Some(style) = style {
            self.mls.modify = Some(Modify {
                handle,
                backup: None,
                previous_selected: previous,
                tab: 0,
                max_points_on: style.max_leader_points > 0,
                first_angle_on: style.first_segment_angle != 0.0,
                second_angle_on: style.second_segment_angle != 0.0,
                scale_to_layout: style.scale_factor == 0.0 && !style.is_annotative,
            });
        }
        self.load_mleaderstyle_bufs(i);
        Task::none()
    }

    fn insert_mleader_style_default(&mut self, name: &str, handle: Handle) {
        let mut style = MultiLeaderStyle::new(name);
        style.description = name.to_string();
        style.align_space = style.text_height;
        style.handle = handle;
        self.tabs[self.active_tab]
            .scene
            .document
            .objects
            .insert(handle, ObjectType::MultiLeaderStyle(style));
    }

    fn mleader_style_ok(&mut self) -> Task<Message> {
        if let Err(field) = self.mleader_style_stage_bufs() {
            // OK is refused without a message: the field shows its value
            // selected.
            if let Some(m) = self.mls.modify.as_mut() {
                m.tab = field_tab(field);
            }
            let id = field_id(field);
            return Task::batch([
                iced::widget::operation::focus(id.clone()),
                iced::widget::operation::select_all(id),
            ]);
        }
        let i = self.active_tab;
        // Block content without a block takes the default source block.
        let needs_block = self
            .mleaderstyle_ref(i)
            .is_some_and(|s| s.content_type == LeaderContentType::Block && s.block_content_handle.is_none());
        if needs_block {
            let doc = &mut self.tabs[i].scene.document;
            let block = crate::entities::leader_blocks::resolve(
                doc,
                crate::entities::leader_blocks::STANDARD[0].0,
            );
            if let Some(s) = self.mleaderstyle_mut(i) {
                s.block_content_handle = block;
            }
        }
        let Some(m) = self.mls.modify.take() else {
            return Task::none();
        };
        self.mls.pick = None;
        self.mls_color_open = None;
        // A new style becomes the current one.
        if m.backup.is_none() {
            self.mleader_style_set_current();
        }
        self.after_style_change(StyleKind::MLeader);
        self.load_mleaderstyle_bufs(i);
        Task::none()
    }

    /// Text style "…": the Text Style manager opens as a child of this
    /// window's transaction, so the whole session stays one undo step.
    fn mleader_style_open_text_styles(&mut self) -> Task<Message> {
        if self.mls.modify.is_none() {
            return Task::none();
        }
        self.mls_color_open = None;
        // The Text Style manager opens on the style's text style, made the
        // drawing's current one (it stays current after the manager closes).
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let style_text = self
            .mleaderstyle_ref(i)
            .and_then(|s| s.text_style_handle)
            .and_then(|h| doc.text_styles.iter().find(|t| t.handle == h))
            .map(|t| t.name.clone());
        if let Some(name) = style_text {
            self.tabs[i].scene.document.header.current_text_style_name = name;
            self.sync_ribbon_styles();
        }
        let parent = self.style_stage.take();
        let task = self.on_text_style_dialog_open();
        self.style_parent_stage = parent;
        task
    }
}
