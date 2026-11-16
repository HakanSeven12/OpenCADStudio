//! The Text Editor ribbon tab and the open editor: what the tab shows of the
//! editor after every update, and the `_MTED <action>` tools it runs.

use super::*;
use crate::app::{Message, OpenCADStudio};
use crate::ui::ribbon::text_editor::TextEditorRibbon;
use document::entities::mtext_format::MTextParagraphAlignment;
use iced::Task;

/// The `_MTED` tool of each character format.
const FORMATS: [(MTextFmt, &str); 5] = [
    (MTextFmt::Bold, "_MTED BOLD"),
    (MTextFmt::Italic, "_MTED ITALIC"),
    (MTextFmt::Underline, "_MTED UNDERLINE"),
    (MTextFmt::Overline, "_MTED OVERLINE"),
    (MTextFmt::Strike, "_MTED STRIKE"),
];

impl OpenCADStudio {
    /// What the Text Editor tab shows of the open editor, or None while no
    /// multiline text is edited.
    pub(in crate::app) fn text_editor_ribbon_state(&self) -> Option<TextEditorRibbon> {
        let ed = self.mtext_editor.as_ref()?;
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let mut lit = Vec::new();
        let mut off = Vec::new();
        for (kind, id) in FORMATS {
            match ed.format_state(kind) {
                FormatState::On => lit.push(id),
                FormatState::Unavailable => off.push(id),
                FormatState::Off | FormatState::Mixed => {}
            }
        }
        if ed.annotative {
            lit.push("_MTED ANNOTATIVE");
        }
        if ed.mask().on {
            lit.push("_MTED MASK");
        }
        lit.push(match ed.caret_alignment() {
            None | Some(MTextParagraphAlignment::Default) => "_MTED ALIGN DEFAULT",
            Some(MTextParagraphAlignment::Left) => "_MTED ALIGN LEFT",
            Some(MTextParagraphAlignment::Center) => "_MTED ALIGN CENTER",
            Some(MTextParagraphAlignment::Right) => "_MTED ALIGN RIGHT",
            Some(MTextParagraphAlignment::Justified) => "_MTED ALIGN JUSTIFY",
            Some(MTextParagraphAlignment::Distributed) => "_MTED ALIGN DISTRIBUTE",
        });
        if ed.ruler {
            lit.push("_MTED RULER");
        }
        if ed.auto_caps {
            lit.push("_MTED AUTOCAPS");
        }
        if ed.selected_range().is_empty() {
            off.extend(["_MTED STACK", "_MTED SUPERSCRIPT", "_MTED SUBSCRIPT"]);
        }
        let font = {
            let font = ed.font_field();
            match &ed.object_style.font {
                Some(style_font) if font.trim().is_empty() => style_font.label().into_owned(),
                _ => crate::ui::font_picker::FontChoice::of_name(&font)
                    .label()
                    .into_owned(),
            }
        };
        let mut styles: Vec<String> = doc.text_styles.iter().map(|s| s.name.clone()).collect();
        styles.sort_by_key(|name| name.to_lowercase());
        Some(TextEditorRibbon {
            styles,
            style: ed.style.clone(),
            height: ed.span_field("height", &ed.height).to_string(),
            font,
            color: ed.whole_text_color(),
            color_open: ed.color_picker_open,
            layer: self.text_layer(i).unwrap_or_default(),
            layers: doc.layers.iter().map(|layer| layer.name.clone()).collect(),
            oblique: ed.span_field("oblique", &ed.oblique).to_string(),
            tracking: ed.char_space.clone(),
            width: ed.span_field("width", &ed.width).to_string(),
            lit,
            off,
        })
    }

    /// Hand the ribbon what the open editor holds, and show or leave the Text
    /// Editor tab with it.
    pub(in crate::app) fn sync_text_editor_ribbon(&mut self) {
        let state = self.text_editor_ribbon_state();
        let editing = state.is_some();
        if crate::ui::ribbon::text_editor::set(state) {
            let doc = &self.tabs[self.active_tab].scene.document;
            crate::ui::ribbon::text_editor::set_fonts(
                crate::ui::font_picker::catalogue(doc, true)
                    .into_iter()
                    .map(|entry| entry.choice)
                    .collect(),
            );
        }
        self.ribbon.set_text_editor_context(editing);
    }

    /// Run a Text Editor tool (`_MTED <action>`) on the open editor. None
    /// for any other command; with no editor open the tool does nothing.
    pub(in crate::app) fn dispatch_text_editor_tool(&mut self, cmd: &str) -> Option<Task<Message>> {
        let rest = cmd.trim().strip_prefix("_MTED")?.trim().to_string();
        if self.mtext_editor.is_none() {
            return Some(Task::none());
        }
        let (action, arg) = rest.split_once(' ').unwrap_or((rest.as_str(), ""));
        let arg = arg.trim();
        let message = match action {
            "ANNOTATIVE" => {
                let on = self.mtext_editor.as_ref().is_some_and(|ed| ed.annotative);
                Message::MTextAnnotative(!on)
            }
            "BOLD" => Message::MTextFmt(MTextFmt::Bold),
            "ITALIC" => Message::MTextFmt(MTextFmt::Italic),
            "UNDERLINE" => Message::MTextFmt(MTextFmt::Underline),
            "OVERLINE" => Message::MTextFmt(MTextFmt::Overline),
            "STRIKE" => Message::MTextFmt(MTextFmt::Strike),
            "CLEAR" => Message::MTextClearFormatting,
            "STACK" => Message::MTextStack,
            "SUPERSCRIPT" => Message::MTextScript(true),
            "SUBSCRIPT" => Message::MTextScript(false),
            "CASE" => Message::MTextFmt(if arg == "LOWER" {
                MTextFmt::Lowercase
            } else {
                MTextFmt::Uppercase
            }),
            "JUSTIFY" => Message::MTextJustify(match arg {
                "TC" => AttachmentPoint::TopCenter,
                "TR" => AttachmentPoint::TopRight,
                "ML" => AttachmentPoint::MiddleLeft,
                "MC" => AttachmentPoint::MiddleCenter,
                "MR" => AttachmentPoint::MiddleRight,
                "BL" => AttachmentPoint::BottomLeft,
                "BC" => AttachmentPoint::BottomCenter,
                "BR" => AttachmentPoint::BottomRight,
                _ => AttachmentPoint::TopLeft,
            }),
            "ALIGN" => Message::MTextAlign(match arg {
                "LEFT" => ParaAlign::Left,
                "CENTER" => ParaAlign::Center,
                "RIGHT" => ParaAlign::Right,
                "JUSTIFY" => ParaAlign::Justify,
                "DISTRIBUTE" => ParaAlign::Distribute,
                _ => ParaAlign::Default,
            }),
            "LIST" => match arg {
                "OFF" => Message::MTextList(None),
                "NUMBER" => Message::MTextList(Some(ListKind::Number)),
                "BULLET" => Message::MTextList(Some(ListKind::Bullet)),
                _ => return Some(self.text_editor_tool_unavailable(cmd)),
            },
            "SPACING" => match arg.parse::<f64>() {
                Ok(factor) => Message::MTextLineSpacing(factor),
                Err(_) => Message::MTextLineSpacing(1.0),
            },
            "COLUMNS" => {
                let mut parts = arg.split_whitespace();
                let mode = match (parts.next(), parts.next()) {
                    (Some("DYNAMIC"), Some("MANUAL")) => "Dynamic manual",
                    (Some("DYNAMIC"), _) => "Dynamic auto",
                    (Some("STATIC"), count) => {
                        let _ = self.update(Message::MTextColumnMode("Static".to_string()));
                        return Some(self.update(Message::MTextColumnCount(
                            count.unwrap_or("2").to_string(),
                        )));
                    }
                    _ => "No columns",
                };
                Message::MTextColumnMode(mode.to_string())
            }
            "SYMBOL" => Message::MTextInsert(symbol_text(arg)),
            "FIELD" => Message::MTextInsertField,
            "AUTOCAPS" => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.auto_caps = !ed.auto_caps;
                }
                return Some(Task::none());
            }
            "RULER" => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.ruler = !ed.ruler;
                }
                return Some(Task::none());
            }
            "CHARMAP" => {
                open_character_map();
                return Some(Task::none());
            }
            "TOOLBAR" => {
                let on = crate::app::commands::mtext_toolbar() != 1;
                crate::app::commands::set_mtext_toolbar(if on { 1 } else { 2 });
                self.save_config();
                return Some(Task::none());
            }
            "UNDO" => Message::MTextUndo,
            "REDO" => Message::MTextRedo,
            "CLOSE" => Message::MTextOk,
            "MASK" | "PARAGRAPH" | "COLUMNSETTINGS" | "FIND" => {
                self.open_mtext_dialog(action);
                return Some(Task::none());
            }
            _ => return Some(self.text_editor_tool_unavailable(cmd)),
        };
        Some(self.update(message))
    }

    /// A Text Editor tool this build does not carry out yet.
    fn text_editor_tool_unavailable(&mut self, cmd: &str) -> Task<Message> {
        self.command_line
            .push_info(&crate::tf!("{cmd} is not available in the text editor yet."));
        Task::none()
    }
}

/// The text a Symbol menu entry puts in: `%%d`, `%%p` and `%%c` as their
/// characters, `\U+XXXX` as that character, NBSP as a non-breaking space.
fn symbol_text(code: &str) -> String {
    match code {
        "%%d" => "°".to_string(),
        "%%p" => "±".to_string(),
        "%%c" => "∅".to_string(),
        "NBSP" => "\u{a0}".to_string(),
        code => code
            .strip_prefix("\\U+")
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .and_then(char::from_u32)
            .map_or_else(|| code.to_string(), String::from),
    }
}

/// Other…: the system's character map, from which a character is copied and
/// pasted into the text.
fn open_character_map() {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("charmap.exe").spawn();
    }
}
