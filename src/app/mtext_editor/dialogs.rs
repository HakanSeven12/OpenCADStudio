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

fn parse(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

impl OpenCADStudio {
    /// Open the Paragraph, Column Settings, Background Mask or Find and
    /// Replace window (`PARAGRAPH`, `COLUMNSETTINGS`, `MASK`, `FIND`) on the
    /// open editor.
    pub(in crate::app) fn open_mtext_dialog(&mut self, which: &str) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let dialog = match which {
            "PARAGRAPH" => {
                let props = ed.caret_paragraph_props();
                let left = props.left_margin.unwrap_or(0.0);
                let first = left + props.first_line_indent.unwrap_or(0.0);
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
                        (kind, tab.position())
                    })
                    .collect();
                let (line_on, line_mode, line_at) = match props.line_spacing {
                    Some(MTextLineSpacing::Exact(v)) => (true, LineMode::Exactly, v),
                    Some(MTextLineSpacing::Multiple(v)) => (true, LineMode::Multiple, v),
                    _ => (false, LineMode::Multiple, 1.0),
                };
                MTextDialog::Paragraph(ParagraphDraft {
                    tab_kind: TabKind::Left,
                    tab_position: "0".to_string(),
                    tabs,
                    selected_tab: None,
                    decimal: '.',
                    first_line: number(first),
                    hanging: number(left),
                    right: number(props.right_margin.unwrap_or(0.0)),
                    align_on: props.alignment.is_some(),
                    align: match props.alignment {
                        Some(MTextParagraphAlignment::Center) => Align::Center,
                        Some(MTextParagraphAlignment::Right) => Align::Right,
                        Some(MTextParagraphAlignment::Justified) => Align::Justified,
                        Some(MTextParagraphAlignment::Distributed) => Align::Distributed,
                        _ => Align::Left,
                    },
                    spacing_on: props.spacing_before.is_some() || props.spacing_after.is_some(),
                    before: number(props.spacing_before.unwrap_or(0.0)),
                    after: number(props.spacing_after.unwrap_or(0.0)),
                    line_on,
                    line_mode,
                    line_at: format!("{}x", number(line_at)),
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
                let mask = ed.mask();
                MTextDialog::Mask(MaskDraft {
                    on: mask.on,
                    factor: mask.scale.clone(),
                    background: mask.window,
                    color: mask.color,
                })
            }
            _ => MTextDialog::Find(FindDraft {
                find: ed.find_text.clone(),
                replace: ed.replace_text.clone(),
                diacritics: true,
                ..Default::default()
            }),
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
                let hanging = parse(&d.hanging).unwrap_or(0.0);
                let first = parse(&d.first_line).unwrap_or(0.0);
                self.mtext_apply_paragraph_number(ParaNumber::LeftIndent, number(hanging));
                self.mtext_apply_paragraph_number(ParaNumber::FirstIndent, number(first - hanging));
                self.mtext_apply_paragraph_number(
                    ParaNumber::RightIndent,
                    number(parse(&d.right).unwrap_or(0.0)),
                );
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
                    self.mtext_apply_paragraph_number(
                        ParaNumber::SpaceBefore,
                        number(parse(&d.before).unwrap_or(0.0)),
                    );
                    self.mtext_apply_paragraph_number(
                        ParaNumber::SpaceAfter,
                        number(parse(&d.after).unwrap_or(0.0)),
                    );
                }
                if d.line_on {
                    let at = parse(d.line_at.trim().trim_end_matches(['x', 'X'])).unwrap_or(1.0);
                    self.raw_paragraph_line_spacing(match d.line_mode {
                        LineMode::Exactly => MTextLineSpacing::Exact(at),
                        LineMode::AtLeast | LineMode::Multiple => MTextLineSpacing::Multiple(at),
                    });
                }
                let tabs: Vec<TabStop> = d
                    .tabs
                    .iter()
                    .map(|(kind, position)| match kind {
                        TabKind::Left => TabStop::Left(*position),
                        TabKind::Center => TabStop::Center(*position),
                        TabKind::Right => TabStop::Right(*position),
                        TabKind::Decimal => TabStop::Decimal(*position),
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
        let found = matches
            .iter()
            .find(|(s, _)| *s >= from)
            .or_else(|| matches.first())
            .copied();
        if let Some((start, end)) = found {
            ed.sel_anchor = start;
            ed.sel = Some((start, end));
            ed.caret = end;
            ed.caret_blink_on = true;
        }
        let notice = found.is_none().then(|| crate::t!("The text was not found.").into_owned());
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
            draft.notice = Some(if count == 0 {
                crate::t!("The text was not found.").into_owned()
            } else {
                crate::tf!("{count} occurrence(s) replaced.").into_owned()
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

/// A character as the find options compare it.
fn folded(c: char, d: &FindDraft) -> char {
    let mut c = c;
    if !d.half_full && ('\u{FF01}'..='\u{FF5E}').contains(&c) {
        c = char::from_u32(c as u32 - 0xFEE0).unwrap_or(c);
    }
    if !d.diacritics {
        c = base_letter(c);
    }
    if !d.match_case {
        c = c.to_lowercase().next().unwrap_or(c);
    }
    c
}

/// The letter without its diacritic, for the Latin letters text uses most.
fn base_letter(c: char) -> char {
    const FROM: &str = "ÀÁÂÃÄÅàáâãäåÇçĆćČčÈÉÊËèéêëĞğÌÍÎÏìíîïİıÑñŃńÒÓÔÕÖØòóôõöøŞşŠšŚśÙÚÛÜùúûüÝýÿŽžŹźŻż";
    const TO: &str = "AAAAAAaaaaaaCcCcCcEEEEeeeeGgIIIIiiiiIiNnNnOOOOOOooooooSsSsSsUUUUuuuuYyyZzZzZz";
    FROM.chars()
        .position(|f| f == c)
        .and_then(|n| TO.chars().nth(n))
        .unwrap_or(c)
}

fn is_word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// Every match of the find text in `chars` as (start, end), left to right and
/// not overlapping. With wildcards `*` stands for any run and `?` for any one
/// character (`#` a digit, `@` a letter).
fn find_matches(chars: &[Option<char>], d: &FindDraft) -> Vec<(usize, usize)> {
    let needle: Vec<char> = d.find.chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let end = if d.wildcards {
            wild_match(chars, at, &needle, d)
        } else {
            (at + needle.len() <= chars.len()
                && chars[at..at + needle.len()]
                    .iter()
                    .zip(&needle)
                    .all(|(c, n)| c.is_some_and(|c| folded(c, d) == folded(*n, d))))
            .then_some(at + needle.len())
        };
        match end {
            Some(end)
                if end > at
                    && (!d.whole_words
                        || (!is_word(at.checked_sub(1).and_then(|p| chars[p]))
                            && !is_word(chars.get(end).copied().flatten()))) =>
            {
                out.push((at, end));
                at = end;
            }
            _ => at += 1,
        }
    }
    out
}

/// The end of the shortest wildcard match of `pattern` starting at `at`.
fn wild_match(chars: &[Option<char>], at: usize, pattern: &[char], d: &FindDraft) -> Option<usize> {
    fn go(chars: &[Option<char>], i: usize, p: &[char], d: &FindDraft) -> Option<usize> {
        let Some((&first, rest)) = p.split_first() else {
            return Some(i);
        };
        match first {
            '*' => (i..=chars.len()).find_map(|j| go(chars, j, rest, d)),
            _ => {
                let c = chars.get(i).copied().flatten()?;
                let ok = match first {
                    '?' => c != '\n',
                    '#' => c.is_ascii_digit(),
                    '@' => c.is_alphabetic(),
                    other => folded(c, d) == folded(other, d),
                };
                if ok {
                    go(chars, i + 1, rest, d)
                } else {
                    None
                }
            }
        }
    }
    go(chars, at, pattern, d)
}
