//! Superscript, subscript and fractions, all stored as standard MTEXT stacks:
//! `\S12^;` raises, `\S^12;` lowers, `\S1/2;`, `\S1#2;` and `\S1^2;` stack
//! two parts. A stack is one atom: it is created, replaced or unstacked whole.
use super::raw_adapter::{EditError, Kind, SourceIndex};
use super::raw_edit::editor_document;
use super::raw_operations::{apply_patches, TextEdit};
use super::*;
use crate::app::OpenCADStudio;
use std::ops::Range;

/// Raised or lowered text: a one-sided `^` stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Super,
    Sub,
}

impl Script {
    fn stack(self, text: &str) -> StackingData {
        let (numerator, denominator) = match self {
            Self::Super => (text.to_owned(), String::new()),
            Self::Sub => (String::new(), text.to_owned()),
        };
        StackingData {
            numerator,
            denominator,
            stacking_type: StackingType::Limit,
        }
    }

    /// The script a stack draws as, when it is one-sided.
    fn of(data: &StackingData) -> Option<Self> {
        if data.stacking_type != StackingType::Limit {
            return None;
        }
        match (data.numerator.is_empty(), data.denominator.is_empty()) {
            (false, true) => Some(Self::Super),
            (true, false) => Some(Self::Sub),
            _ => None,
        }
    }
}

/// The Fraction panel's editable parts, never part of the undo history.
#[derive(Clone, Debug, PartialEq)]
pub struct FractionDraft {
    pub numerator: String,
    pub denominator: String,
    pub kind: StackingType,
}

/// Which part of the Fraction panel a field edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FractionPart {
    Numerator,
    Denominator,
}

/// A stack part holds plain characters only: the stack's own separators,
/// its terminator, escapes, groups and percent codes cannot be stored in it.
fn storable(part: &str) -> bool {
    !part
        .chars()
        .any(|c| matches!(c, '/' | '#' | '^' | '%' | ';' | '\\' | '{' | '}') || c.is_control())
}

/// The `\S` code of a stack. The `^` carries no space: in a DXF file a bare
/// caret is written as `^ ` (AutoCAD's on-disk `\S2^ ;`) and read back as `^`,
/// so only this form reads back byte-identical from both DWG and DXF.
fn stack_code(data: &StackingData) -> Result<String, EditError> {
    // After `^` the readers skip one space, so a lower part cannot start
    // with one.
    if !storable(&data.numerator)
        || !storable(&data.denominator)
        || (data.stacking_type == StackingType::Limit && data.denominator.starts_with(' '))
    {
        return Err(EditError::StackText);
    }
    let (sep, both) = match data.stacking_type {
        StackingType::Horizontal => ("/", true),
        StackingType::Diagonal => ("#", true),
        StackingType::Limit => ("^", false),
    };
    let empty = |s: &str| s.trim().is_empty();
    if (both && (empty(&data.numerator) || empty(&data.denominator)))
        || (empty(&data.numerator) && empty(&data.denominator))
    {
        return Err(EditError::FractionParts);
    }
    Ok(format!("\\S{}{sep}{};", data.numerator, data.denominator))
}

impl SourceIndex<'_> {
    /// Replace the visible `range` (empty: insert at it) with one stack. The
    /// range may hold plain characters or exactly one existing stack; every
    /// other source token, inside it or not, is kept verbatim.
    pub fn stack(&self, range: Range<usize>, data: &StackingData) -> Result<TextEdit, EditError> {
        self.validate_range(&range)?;
        let code = stack_code(data)?;
        let selected: Vec<_> = self
            .units
            .iter()
            .filter(|u| u.visible.start < range.end && u.visible.end > range.start)
            .collect();
        let stacks = selected.iter().filter(|u| u.kind == Kind::Stack).count();
        for unit in &selected {
            if unit.visible.start < range.start || unit.visible.end > range.end {
                return Err(EditError::AtomicStructure);
            }
            let plain = matches!(
                unit.kind,
                Kind::Text | Kind::Escape | Kind::Unicode | Kind::Percent
            );
            if !(plain || (unit.kind == Kind::Stack && selected.len() == 1)) {
                return Err(if stacks > 0 {
                    EditError::AtomicStructure
                } else {
                    EditError::StackText
                });
            }
        }
        let mut patches: Vec<_> = selected
            .iter()
            .map(|u| (u.raw.clone(), String::new()))
            .collect();
        match patches.first_mut() {
            Some(first) => first.1 = code.clone(),
            None => {
                let at = self.insertion_byte(range.start)?;
                patches.push((at..at, code.clone()));
            }
        }
        let width = data.numerator.chars().count()
            + usize::from(!data.denominator.is_empty())
            + data.denominator.chars().count();
        let raw = apply_patches(self.raw, patches)?;
        self.certify(&raw, self.visible_len.unwrap_or(0) - range.len() + width)?;
        Ok(TextEdit {
            raw,
            caret: range.start + width,
        })
    }
}

impl MTextEditorState {
    /// The selection's one stack, when it selects exactly one.
    fn selected_stack(&self) -> Option<StackingData> {
        let range = self.selected_range();
        match doc_to_cells(&self.doc).get(range.start..range.end)? {
            [Cell::Stack {
                data, head: true, ..
            }, rest @ ..]
                if rest
                    .iter()
                    .all(|c| matches!(c, Cell::Stack { head: false, .. })) =>
            {
                Some(data.clone())
            }
            _ => None,
        }
    }

    /// Whether the selection is exactly one stack raised or lowered this way:
    /// the menu's check mark. Super- and subscript exclude each other.
    pub(in crate::app) fn script_active(&self, script: Script) -> bool {
        self.selected_stack()
            .is_some_and(|data| Script::of(&data) == Some(script))
    }

    /// The panel's starting values: the selected stack, or a plain `a/b`
    /// selection split at its one slash. Nothing else is guessed.
    pub(in crate::app) fn fraction_draft(&self) -> FractionDraft {
        if let Some(data) = self.selected_stack() {
            return FractionDraft {
                numerator: data.numerator,
                denominator: data.denominator,
                kind: data.stacking_type,
            };
        }
        let empty = FractionDraft {
            numerator: String::new(),
            denominator: String::new(),
            kind: StackingType::Horizontal,
        };
        match selected_plain(self).map(|t| t.split('/').map(str::to_owned).collect::<Vec<_>>()) {
            Some(parts) if parts.len() == 2 => FractionDraft {
                numerator: parts[0].clone(),
                denominator: parts[1].clone(),
                ..empty
            },
            _ => empty,
        }
    }
}

impl OpenCADStudio {
    fn stack_error(&mut self, error: EditError) {
        self.command_line
            .push_error(crate::t!(error.to_string()).as_ref());
    }

    /// Raise or lower the selection. On a selection that already is this
    /// index the text is unstacked again; on the other index it is swapped.
    pub(in crate::app) fn mtext_script(&mut self, script: Script) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let range = ed.selected_range();
        if range.is_empty() {
            return;
        }
        match ed.selected_stack() {
            Some(data) if Script::of(&data) == Some(script) => {
                let text = data.numerator + &data.denominator;
                self.raw_unstack(range, text);
            }
            Some(data) => match Script::of(&data) {
                Some(_) => {
                    let text = data.numerator + &data.denominator;
                    self.raw_stack(range, script.stack(&text));
                }
                None => self.stack_error(EditError::AtomicStructure),
            },
            None => match selected_plain(ed) {
                Some(text) => {
                    self.raw_stack(range, script.stack(&text));
                }
                None => self.stack_error(EditError::StackText),
            },
        }
    }

    /// Stack the selected text at its first `/`, `#` or `^`. A `^` stack may
    /// leave one half empty: "2^" stacks as a superscript and "^2" as a
    /// subscript (#2236).
    pub(in crate::app) fn mtext_stack(&mut self) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let range = ed.selected_range();
        if range.is_empty() {
            return;
        }
        let Some(plain) = selected_plain(ed) else {
            self.stack_error(EditError::StackText);
            return;
        };
        let Some((split, separator)) = plain
            .char_indices()
            .find(|(_, ch)| matches!(ch, '/' | '#' | '^'))
        else {
            self.stack_error(EditError::FractionParts);
            return;
        };
        let data = StackingData {
            numerator: plain[..split].to_string(),
            denominator: plain[split + separator.len_utf8()..].to_string(),
            stacking_type: StackingType::from_char(separator),
        };
        self.raw_stack(range, data);
    }

    /// Open the Fraction panel with the selection's values; its own row
    /// closes it again, back to the More menu.
    pub(in crate::app) fn mtext_fraction_open(&mut self) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            if ed.menu == Some(EditorMenu::Fraction) {
                ed.menu = Some(EditorMenu::More);
                ed.fraction = None;
                return;
            }
            ed.fraction = Some(ed.fraction_draft());
            ed.menu = Some(EditorMenu::Fraction);
        }
    }

    pub(in crate::app) fn mtext_fraction_part(&mut self, part: FractionPart, value: String) {
        if let Some(draft) = self
            .mtext_editor
            .as_mut()
            .and_then(|ed| ed.fraction.as_mut())
        {
            match part {
                FractionPart::Numerator => draft.numerator = value,
                FractionPart::Denominator => draft.denominator = value,
            }
        }
    }

    pub(in crate::app) fn mtext_fraction_kind(&mut self, kind: StackingType) {
        if let Some(draft) = self
            .mtext_editor
            .as_mut()
            .and_then(|ed| ed.fraction.as_mut())
        {
            draft.kind = kind;
        }
    }

    /// Insert the panel's fraction over the selection (or at the caret), or
    /// replace the selected stack with it. A refused draft keeps the panel.
    pub(in crate::app) fn mtext_fraction_insert(&mut self) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let Some(draft) = ed.fraction.clone() else {
            return;
        };
        let data = StackingData {
            numerator: draft.numerator,
            denominator: draft.denominator,
            stacking_type: draft.kind,
        };
        if let Err(error) = stack_code(&data) {
            self.stack_error(error);
            return;
        }
        let range = ed.selected_range();
        if self.raw_stack(range, data) {
            let ed = self.mtext_editor.as_mut().unwrap();
            ed.fraction = None;
            ed.menu = None;
        }
    }

    /// Replace `range` with one stack in a single undo step and select it.
    /// The characters around it and the stack's own formatting are certified.
    fn raw_stack(&mut self, range: Range<usize>, data: StackingData) -> bool {
        let before = self.mtext_editor.as_ref().map(|ed| ed.content.text());
        self.edit_raw(|ed, index| {
            let cells = doc_to_cells(&ed.doc);
            let selected = cells.get(range.clone()).ok_or(EditError::InvalidRange)?;
            let props = uniform_props(selected)?;
            let edit = index.stack(range.clone(), &data)?;
            let after = doc_to_cells(&editor_document(&edit.raw));
            let width = edit.caret - range.start;
            let made = after.get(range.start..edit.caret).unwrap_or(&[]);
            let kept = after.get(..range.start) == cells.get(..range.start)
                && after.get(edit.caret..) == cells.get(range.end..);
            let stacked = made.len() == width
                && made.iter().all(|c| match c {
                    Cell::Stack {
                        data: d, props: p, ..
                    } => d == &data && props.as_ref().is_none_or(|props| p == props),
                    _ => false,
                });
            if !(kept && stacked) {
                return Err(EditError::ProjectionMismatch);
            }
            Ok(edit)
        });
        self.select_after_edit(before, range.start)
    }

    /// Turn the selected one-sided stack back into its plain text.
    fn raw_unstack(&mut self, range: Range<usize>, text: String) {
        let before = self.mtext_editor.as_ref().map(|ed| ed.content.text());
        self.edit_raw(|_, index| index.replace(range.clone(), &text));
        self.select_after_edit(before, range.start);
    }

    /// After an applied edit, select from `start` to the new caret, so the
    /// same menu entry can be used again on the result.
    fn select_after_edit(&mut self, before: Option<String>, start: usize) -> bool {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return false;
        };
        if before.is_none_or(|before| before == ed.content.text()) {
            return false;
        }
        ed.sel = Some((start, ed.caret));
        ed.sel_anchor = start;
        true
    }
}

/// The selected characters as one plain string, when they are all characters.
fn selected_plain(ed: &MTextEditorState) -> Option<String> {
    doc_to_cells(&ed.doc)
        .get(ed.selected_range())?
        .iter()
        .map(|c| match c {
            Cell::Char(ch, _) => Some(*ch),
            _ => None,
        })
        .collect()
}

/// A stack has one formatting: the selection must share it (empty: none).
fn uniform_props(cells: &[Cell]) -> Result<Option<SpanProperties>, EditError> {
    let mut props = cells.iter().map(|c| match c {
        Cell::Char(_, p) | Cell::Stack { props: p, .. } => Ok(p),
        Cell::Break(..) => Err(EditError::StackText),
    });
    let Some(first) = props.next().transpose()? else {
        return Ok(None);
    };
    for p in props {
        if p? != first {
            return Err(EditError::StackFormat);
        }
    }
    Ok(Some(first.clone()))
}
