//! The windows the Text Editor tab opens on the open editor: what each starts
//! from, what its controls change and what OK writes into the text.

use super::*;
use crate::app::{Message, ModalKind, OpenCADStudio};
use crate::ui::window::mtext_dialogs::{
    Align, ColumnKind, ColumnsDraft, FindDraft, LineMode, MTextDialog, MTextDialogMsg, MaskDraft,
    ParagraphDraft, TabKind,
};
use document::entities::mtext_format::{MTextLineSpacing, MTextParagraphAlignment, TabStop};
use iced::Task;

fn number(value: f64) -> String {
    raw_span::number(value)
}

/// A value as the text writes it: six decimals at most.
fn round(value: f64) -> f64 {
    (value * 1e6).round() / 1e6
}

fn parse(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

impl OpenCADStudio {
    /// Open the Paragraph, Column Settings, Background Mask or Find and
    /// Replace window (`PARAGRAPH`, `COLUMNSETTINGS`, `MASK`, `FIND`) on the
    /// open editor.
    pub(in crate::app) fn open_mtext_dialog(&mut self, which: &str) {
        if matches!(
            which,
            "CHARSET" | "DICTIONARIES" | "SPELLSETTINGS" | "HIGHLIGHT" | "STACKPROPS" | "AUTOSTACK"
        ) {
            if let Some(dialog) = self.mtext_tool_dialog(which) {
                self.mtext_dialog = Some(dialog);
                self.active_modal = Some(ModalKind::MTextDialog);
            }
            return;
        }
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let dialog = match which {
            "PARAGRAPH" => {
                // The text stores indents, spacing and tab stops in text
                // heights; the window shows drawing units.
                let props = ed.caret_paragraph_props();
                let h = ed.height_value().max(f64::EPSILON);
                let left = props.left_margin.unwrap_or(0.0) * h;
                let first = left + props.first_line_indent.unwrap_or(0.0) * h;
                let decimal = props
                    .tab_stops
                    .iter()
                    .find_map(|tab| match tab {
                        TabStop::DecimalWithCharacter { character, .. } => Some(*character),
                        _ => None,
                    })
                    .unwrap_or('.');
                let tabs = props
                    .tab_stops
                    .iter()
                    .map(|tab| {
                        let kind = match tab {
                            TabStop::Center(_) => TabKind::Center,
                            TabStop::Right(_) => TabKind::Right,
                            TabStop::Decimal(_) | TabStop::DecimalWithCharacter { .. } => {
                                TabKind::Decimal
                            }
                            TabStop::Left(_) => TabKind::Left,
                        };
                        (kind, tab.position() * h)
                    })
                    .collect();
                let (line_on, line_mode, line_at) = match props.line_spacing {
                    Some(MTextLineSpacing::Exact(v)) => (true, LineMode::Exactly, v * h),
                    Some(MTextLineSpacing::AtLeast(v)) => (true, LineMode::AtLeast, v * h),
                    Some(MTextLineSpacing::Multiple(v)) => (true, LineMode::Multiple, v),
                    _ => (false, LineMode::Multiple, 1.0),
                };
                MTextDialog::Paragraph(ParagraphDraft {
                    tab_kind: TabKind::Left,
                    tab_position: "0".to_string(),
                    tabs,
                    selected_tab: None,
                    decimal,
                    first_line: number(first),
                    hanging: number(left),
                    right: number(props.right_margin.unwrap_or(0.0) * h),
                    align_on: props.alignment.is_some(),
                    align: match props.alignment {
                        Some(MTextParagraphAlignment::Center) => Align::Center,
                        Some(MTextParagraphAlignment::Right) => Align::Right,
                        Some(MTextParagraphAlignment::Justified) => Align::Justified,
                        Some(MTextParagraphAlignment::Distributed) => Align::Distributed,
                        _ => Align::Left,
                    },
                    spacing_on: props.spacing_before.is_some() || props.spacing_after.is_some(),
                    before: number(props.spacing_before.unwrap_or(0.0) * h),
                    after: number(props.spacing_after.unwrap_or(0.0) * h),
                    line_on,
                    line_mode,
                    line_at: if line_mode == LineMode::Multiple {
                        format!("{}x", number(line_at))
                    } else {
                        number(line_at)
                    },
                })
            }
            "COLUMNSETTINGS" => MTextDialog::Columns(ColumnsDraft {
                kind: match (ed.column_type, ed.column_auto_height) {
                    (1, _) => ColumnKind::Static,
                    (2, true) => ColumnKind::DynamicAuto,
                    (2, false) => ColumnKind::DynamicManual,
                    _ => ColumnKind::None,
                },
                number: ed.column_count.clone(),
                height: ed.rect_height.clone(),
                width: ed.column_width.clone(),
                gutter: ed.column_gutter.clone(),
            }),
            "MASK" => {
                // A text without a mask starts from red, not the drawing's
                // background colour.
                let mask = ed.mask();
                MTextDialog::Mask(MaskDraft {
                    on: mask.on,
                    factor: mask.scale.clone(),
                    background: mask.on && mask.window,
                    color: if mask.on {
                        mask.color
                    } else {
                        document::types::Color::Index(1)
                    },
                })
            }
            _ => {
                let [match_case, whole_words, wildcards, diacritics, half_full] = self
                    .mtext_find_prefs
                    .unwrap_or([false, false, false, true, false]);
                MTextDialog::Find(FindDraft {
                    find: ed.find_text.clone(),
                    replace: ed.replace_text.clone(),
                    match_case,
                    whole_words,
                    wildcards,
                    diacritics,
                    half_full,
                    notice: None,
                })
            }
        };
        self.mtext_dialog = Some(dialog);
        self.active_modal = Some(ModalKind::MTextDialog);
    }

    pub(in crate::app) fn on_mtext_dialog(&mut self, message: MTextDialogMsg) -> Task<Message> {
        use MTextDialogMsg as M;
        let Some(dialog) = self.mtext_dialog.as_mut() else {
            return Task::none();
        };
        match (dialog, message) {
            (_, M::Ok) => return self.apply_mtext_dialog(),
            (MTextDialog::Paragraph(d), message) => match message {
                M::TabKind(kind) => d.tab_kind = kind,
                M::TabPosition(v) => d.tab_position = v,
                M::TabAdd => {
                    if let Some(position) = parse(&d.tab_position).filter(|p| *p >= 0.0) {
                        d.tabs.retain(|(_, p)| (p - position).abs() > 1e-9);
                        d.tabs.push((d.tab_kind, position));
                        d.tabs.sort_by(|a, b| a.1.total_cmp(&b.1));
                        d.selected_tab = d.tabs.iter().position(|(_, p)| *p == position);
                    }
                }
                M::TabModify => {
                    if let (Some(n), Some(position)) =
                        (d.selected_tab, parse(&d.tab_position).filter(|p| *p >= 0.0))
                    {
                        if let Some(tab) = d.tabs.get_mut(n) {
                            *tab = (d.tab_kind, position);
                        }
                        d.tabs.sort_by(|a, b| a.1.total_cmp(&b.1));
                    }
                }
                M::TabRemove => {
                    if let Some(n) = d.selected_tab.take() {
                        if n < d.tabs.len() {
                            d.tabs.remove(n);
                        }
                    }
                }
                M::TabSelect(n) => {
                    if let Some((kind, position)) = d.tabs.get(n) {
                        d.tab_kind = *kind;
                        d.tab_position = number(*position);
                        d.selected_tab = Some(n);
                    }
                }
                M::Decimal(c) => d.decimal = c,
                M::FirstLine(v) => d.first_line = v,
                M::Hanging(v) => d.hanging = v,
                M::Right(v) => d.right = v,
                M::AlignOn(on) => d.align_on = on,
                M::Align(a) => d.align = a,
                M::SpacingOn(on) => d.spacing_on = on,
                M::Before(v) => d.before = v,
                M::After(v) => d.after = v,
                M::LineOn(on) => d.line_on = on,
                M::LineMode(mode) => d.line_mode = mode,
                M::LineAt(v) => d.line_at = v,
                _ => {}
            },
            (MTextDialog::Columns(d), message) => match message {
                M::ColumnKind(kind) => d.kind = kind,
                M::ColumnNumber(v) => d.number = v,
                M::ColumnHeight(v) => d.height = v,
                M::ColumnWidth(v) => d.width = v,
                M::ColumnGutter(v) => d.gutter = v,
                _ => {}
            },
            (MTextDialog::Mask(d), message) => match message {
                M::MaskOn(on) => d.on = on,
                M::MaskFactor(v) => d.factor = v,
                M::MaskBackground(on) => d.background = on,
                M::MaskColor(color) => d.color = color,
                M::MaskMoreColor => {
                    let color = d.color;
                    return self.update(Message::OpenColorWindow(
                        crate::app::ColorPickTarget::MTextMask,
                        color,
                    ));
                }
                _ => {}
            },
            (MTextDialog::Find(d), message) => match message {
                M::Find(v) => {
                    d.find = v;
                    d.notice = None;
                }
                M::Replace(v) => d.replace = v,
                M::MatchCase(on) => d.match_case = on,
                M::WholeWords(on) => d.whole_words = on,
                M::Wildcards(on) => d.wildcards = on,
                M::Diacritics(on) => d.diacritics = on,
                M::HalfFull(on) => d.half_full = on,
                M::DismissNotice => d.notice = None,
                M::FindNext => {
                    let draft = d.clone();
                    self.mtext_find_with(&draft, false);
                }
                M::ReplaceOne => {
                    let draft = d.clone();
                    self.mtext_find_with(&draft, true);
                }
                M::ReplaceAll => {
                    let draft = d.clone();
                    self.mtext_replace_all_with(&draft);
                }
                _ => {}
            },
            (_, message) => return self.on_mtext_tool_dialog(message),
        }
        if let Some(MTextDialog::Find(d)) = self.mtext_dialog.as_ref() {
            self.mtext_find_prefs = Some([
                d.match_case,
                d.whole_words,
                d.wildcards,
                d.diacritics,
                d.half_full,
            ]);
        }
        Task::none()
    }

    /// OK: write the window's draft into the text and close the window.
    fn apply_mtext_dialog(&mut self) -> Task<Message> {
        let Some(dialog) = self.mtext_dialog.take() else {
            return Task::none();
        };
        self.close_active_modal();
        match dialog {
            MTextDialog::Paragraph(d) => {
                // Drawing units in the window, text heights in the text.
                let h = self
                    .mtext_editor
                    .as_ref()
                    .map_or(1.0, |ed| ed.height_value())
                    .max(f64::EPSILON);
                let units = |value: &str| number(parse(value).unwrap_or(0.0) / h);
                let hanging = parse(&d.hanging).unwrap_or(0.0);
                let first = parse(&d.first_line).unwrap_or(0.0);
                self.mtext_apply_paragraph_number(ParaNumber::LeftIndent, units(&d.hanging));
                self.mtext_apply_paragraph_number(
                    ParaNumber::FirstIndent,
                    number((first - hanging) / h),
                );
                self.mtext_apply_paragraph_number(ParaNumber::RightIndent, units(&d.right));
                if d.align_on {
                    self.mtext_apply_align(match d.align {
                        Align::Left => ParaAlign::Left,
                        Align::Center => ParaAlign::Center,
                        Align::Right => ParaAlign::Right,
                        Align::Justified => ParaAlign::Justify,
                        Align::Distributed => ParaAlign::Distribute,
                    });
                }
                if d.spacing_on {
                    self.mtext_apply_paragraph_number(ParaNumber::SpaceBefore, units(&d.before));
                    self.mtext_apply_paragraph_number(ParaNumber::SpaceAfter, units(&d.after));
                }
                if d.line_on {
                    let at = parse(d.line_at.trim().trim_end_matches(['x', 'X'])).unwrap_or(1.0);
                    // Exactly and at least are distances; a multiple is a
                    // factor.
                    self.raw_paragraph_line_spacing(match d.line_mode {
                        LineMode::Exactly => MTextLineSpacing::Exact(round(at / h)),
                        LineMode::AtLeast => MTextLineSpacing::AtLeast(round(at / h)),
                        LineMode::Multiple => MTextLineSpacing::Multiple(at),
                    });
                }
                let tabs: Vec<TabStop> = d
                    .tabs
                    .iter()
                    .map(|(kind, position)| {
                        let position = round(position / h);
                        match kind {
                            TabKind::Left => TabStop::Left(position),
                            TabKind::Center => TabStop::Center(position),
                            TabKind::Right => TabStop::Right(position),
                            TabKind::Decimal => TabStop::DecimalWithCharacter {
                                position,
                                character: d.decimal,
                            },
                        }
                    })
                    .collect();
                self.raw_paragraph_tabs(&tabs);
            }
            MTextDialog::Columns(d) => {
                let mode = match d.kind {
                    ColumnKind::Static => "Static",
                    ColumnKind::DynamicAuto => "Dynamic auto",
                    ColumnKind::DynamicManual => "Dynamic manual",
                    ColumnKind::None => "No columns",
                };
                let _ = self.update(Message::MTextColumnMode(mode.to_string()));
                if d.kind != ColumnKind::None {
                    let _ = self.update(Message::MTextColumnCount(d.number));
                    let _ = self.update(Message::MTextColumnHeight(d.height));
                    let _ = self.update(Message::MTextColumnWidth(d.width));
                    let _ = self.update(Message::MTextColumnGutter(d.gutter));
                }
            }
            MTextDialog::Mask(d) => {
                let current = self.mtext_editor.as_ref().map(|ed| ed.mask().on);
                if current.is_some_and(|on| on != d.on) {
                    self.mtext_mask_toggle();
                }
                if d.on {
                    self.mtext_mask_scale(d.factor);
                    self.mtext_mask_color((!d.background).then_some(d.color));
                }
            }
            MTextDialog::Find(d) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.find_text = d.find;
                    ed.replace_text = d.replace;
                }
            }
            other => {
                let task = self.apply_mtext_tool_dialog(other);
                self.rebuild_mtext_preview();
                return task;
            }
        }
        self.rebuild_mtext_preview();
        Task::none()
    }

    /// Find the next match of the window's text from the caret (Replace
    /// first puts the replacement over a selected match), and select it.
    fn mtext_find_with(&mut self, d: &FindDraft, replace: bool) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let chars = editor_chars(ed);
        if replace {
            if let Some((start, end)) = ed.sel.filter(|(s, e)| s < e) {
                let picked = find_matches(&chars, d)
                    .into_iter()
                    .any(|(s, e)| s == start && e == end);
                if picked {
                    self.mtext_type(&d.replace);
                }
            }
        }
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        let chars = editor_chars(ed);
        let matches = find_matches(&chars, d);
        let from = ed.caret;
        // Past the last match the search reports it has finished and starts
        // again from the top on the next press.
        let found = matches.iter().find(|(s, _)| *s >= from).copied();
        match found {
            Some((start, end)) => {
                ed.sel_anchor = start;
                ed.sel = Some((start, end));
                ed.caret = end;
            }
            None => {
                ed.sel = None;
                ed.sel_anchor = 0;
                ed.caret = 0;
            }
        }
        ed.caret_blink_on = true;
        let notice = found
            .is_none()
            .then(|| crate::tr!("find", "finished-text"));
        if let Some(MTextDialog::Find(draft)) = self.mtext_dialog.as_mut() {
            draft.notice = notice;
        }
    }

    /// Replace every match at once, as one undo step.
    fn mtext_replace_all_with(&mut self, d: &FindDraft) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let matches = find_matches(&editor_chars(ed), d);
        let count = matches.len();
        if count > 0 {
            self.raw_replace_ranges(&matches, &d.replace);
        }
        if let Some(MTextDialog::Find(draft)) = self.mtext_dialog.as_mut() {
            draft.notice = Some(match count {
                0 => crate::tr!("find", "finished-text"),
                1 => crate::tr!("find", "replaced-one"),
                n => crate::i18n::plain(crate::tr!("find", "replaced-many", count = n.to_string())),
            });
        }
    }
}

/// The editor's visible text as characters, a stack as None.
fn editor_chars(ed: &MTextEditorState) -> Vec<Option<char>> {
    doc_to_cells(&ed.doc)
        .iter()
        .map(|cell| match cell {
            Cell::Char(ch, _) => Some(*ch),
            Cell::Break(..) => Some('\n'),
            Cell::Stack { .. } => None,
        })
        .collect()
}

/// Every match of the window's text in `chars` as (start, end).
fn find_matches(chars: &[Option<char>], d: &FindDraft) -> Vec<(usize, usize)> {
    crate::app::text_match::find_ranges(
        chars,
        &d.find,
        &crate::app::text_match::MatchOptions {
            match_case: d.match_case,
            whole_words: d.whole_words,
            wildcards: d.wildcards,
            diacritics: d.diacritics,
            half_full: d.half_full,
        },
    )
}
