//! Clear only character controls in the selection. A control whose scope lies
//! wholly inside is removed; an inherited scope is split around the selection
//! and re-opened with verbatim copies of its own controls. Nothing is flattened
//! or neutralized by hard-coded defaults. An independent derived-property check
//! certifies every result; anything it cannot prove is refused.
use super::raw_adapter::{EditError, Kind, SourceIndex, Unit};
use super::raw_operations::{apply_patches, is_character_control as character_control};
use super::*;
use codec::entities::mtext_format::SpanProperties;
use std::ops::Range;

fn legacy_boundary(raw: &str, unit: &Unit) -> bool {
    unit.kind == Kind::Unicode || (unit.kind == Kind::Escape && &raw[unit.raw.clone()] == r"\\")
}

/// One formatting scope while splitting: its open token (None = document
/// root), the character controls written directly in it, and whether it also
/// holds a non-character control that a split could move.
struct Scope {
    open: Option<usize>,
    controls: Vec<Range<usize>>,
    other: bool,
}

impl SourceIndex<'_> {
    /// The source with the selection's character formatting removed.
    /// `keep` are source controls that stay as they are even where they
    /// format the selection (none yet; later the object's own defaults).
    pub(super) fn cleared_selection(
        &self,
        range: Range<usize>,
        keep: &[Range<usize>],
    ) -> Result<String, EditError> {
        self.validate_range(&range)?;
        if range.is_empty() {
            return Err(EditError::InvalidRange);
        }
        let mut bases = keep.to_vec();
        bases.sort_by_key(|r| r.start);
        let mut prefix = String::new();
        for r in &bases {
            prefix.push_str(&self.raw[r.clone()]);
        }
        let default = super::raw_edit::editor_document(&format!("{prefix}x")).paragraphs[0].spans
            [0]
        .properties
        .clone();
        // MTEXT stores B/I only inside a named font control: an object font
        // that turns them on leaves no code that could mean "not bold".
        if default.font.as_ref().is_some_and(|f| f.bold || f.italic) {
            return Err(EditError::UnsupportedInput);
        }
        // Prefer the smallest source diff; split inherited scopes only when
        // removing wholly contained controls cannot certify the selection.
        self.removed_contained(&range, &bases)
            .and_then(|raw| self.certified_clear(raw, &range, &default))
            .or_else(|_| {
                let raw = self.split_scopes(&range, &bases)?;
                self.certified_clear(raw, &range, &default)
            })
    }

    fn removed_contained(
        &self,
        range: &Range<usize>,
        bases: &[Range<usize>],
    ) -> Result<String, EditError> {
        let mut closes = Vec::new();
        let mut patches = Vec::new();
        for token in &self.tokens {
            match token.kind {
                Kind::Open => closes.push(
                    self.tokens[token.partner.ok_or(EditError::UnknownStructure)?]
                        .raw
                        .start,
                ),
                Kind::Close => {
                    closes.pop();
                }
                Kind::Control(code) if character_control(code) && !bases.contains(&token.raw) => {
                    let end = closes.last().copied().unwrap_or(self.raw.len());
                    let affected: Vec<_> = self
                        .units
                        .iter()
                        .filter(|u| u.raw.start >= token.raw.end && u.raw.start < end)
                        .collect();
                    if affected
                        .iter()
                        .any(|u| u.visible.start < range.end && u.visible.end > range.start)
                        && affected.iter().all(|u| {
                            u.visible.start >= range.start
                                && u.visible.end <= range.end
                                && matches!(
                                    u.kind,
                                    Kind::Text
                                        | Kind::Escape
                                        | Kind::Unicode
                                        | Kind::Percent
                                        | Kind::Break
                                )
                        })
                    {
                        patches.push((token.raw.clone(), String::new()));
                    }
                }
                _ => {}
            }
        }
        apply_patches(self.raw, patches)
    }

    /// Close every scope that carries character controls just before the
    /// selection, copy the selected units without controls, then re-open the
    /// scopes active at the selection end with verbatim copies of their own
    /// controls. Root-level controls before the selection get one new group so
    /// they stop there; they are re-emitted after it. Scopes that would be
    /// left empty on either side are not written at all.
    fn split_scopes(
        &self,
        range: &Range<usize>,
        bases: &[Range<usize>],
    ) -> Result<String, EditError> {
        let selected: Vec<&Unit> = self
            .units
            .iter()
            .filter(|u| u.visible.start < range.end && u.visible.end > range.start)
            .collect();
        let (Some(first), Some(last)) = (selected.first(), selected.last()) else {
            return Err(EditError::InvalidRange);
        };
        if selected.iter().any(|u| {
            u.visible.start < range.start || u.visible.end > range.end || u.kind == Kind::Column
        }) {
            return Err(EditError::AtomicStructure);
        }
        let (start, end) = (first.raw.start, last.raw.end);
        let mut scopes = vec![Scope {
            open: None,
            controls: vec![],
            other: false,
        }];
        let mut root_first = None;
        let mut at_start: Option<Vec<Option<usize>>> = None;
        let mut middle = String::new();
        let mut next = 0;
        let mut previous: Option<&Unit> = None;
        let mut gap = false;
        for (i, token) in self.tokens.iter().enumerate() {
            if token.raw.start >= end {
                break;
            }
            let inside = token.raw.end > start;
            if inside && at_start.is_none() {
                at_start = Some(scopes.iter().map(|s| s.open).collect());
            }
            match token.kind {
                Kind::Open => {
                    scopes.push(Scope {
                        open: Some(i),
                        controls: vec![],
                        other: false,
                    });
                    gap |= inside;
                }
                Kind::Close => {
                    if scopes.len() < 2 {
                        return Err(EditError::UnknownStructure);
                    }
                    // A scope closed inside the selection is never re-opened,
                    // so a paragraph-level control in it would be moved.
                    if scopes.pop().is_some_and(|s| s.other && inside) {
                        return Err(EditError::ParagraphBoundary);
                    }
                    gap |= inside;
                }
                Kind::Control(_) if bases.contains(&token.raw) => {}
                Kind::Control(code) => {
                    let scope = scopes.last_mut().unwrap();
                    if !character_control(code) {
                        if inside {
                            return Err(EditError::ParagraphBoundary);
                        }
                        scope.other = true;
                    } else {
                        scope.controls.push(token.raw.clone());
                        if !inside && scope.open.is_none() {
                            root_first.get_or_insert(token.raw.start);
                        }
                    }
                    gap |= inside;
                }
                Kind::Column | Kind::Field | Kind::Unknown(_) | Kind::Invalid if inside => {
                    return Err(EditError::AtomicStructure);
                }
                Kind::Ignored if inside => middle.push_str(&self.raw[token.raw.clone()]),
                _ if inside => {
                    while let Some(unit) = selected.get(next).filter(|u| u.raw.end <= token.raw.end)
                    {
                        if gap && previous.is_some_and(|p| legacy_boundary(self.raw, p)) {
                            middle.push_str("{}");
                        }
                        middle.push_str(&self.raw[unit.raw.clone()]);
                        previous = Some(unit);
                        gap = false;
                        next += 1;
                    }
                }
                _ => {}
            }
        }
        if next != selected.len() {
            return Err(EditError::ProjectionMismatch);
        }
        let at_start = at_start.ok_or(EditError::InvalidRange)?;
        // Groups open across the whole selection with no character control up
        // to its end stay as they are. A root control rules this out: one
        // re-emitted after the selection must not be trapped in such a group.
        let keep = if scopes[0].controls.is_empty() {
            at_start[1..]
                .iter()
                .zip(&scopes[1..])
                .take_while(|(a, s)| **a == s.open && s.controls.is_empty() && !s.other)
                .count()
        } else {
            0
        };
        if scopes[1 + keep..].iter().any(|s| s.other) {
            return Err(EditError::ParagraphBoundary);
        }
        // Scopes to close before the selection, outer to inner, by the raw
        // position they start at: the new root wrapper first, then groups.
        let mut closing: Vec<usize> = root_first.into_iter().collect();
        closing.extend(
            at_start[1 + keep..]
                .iter()
                .map(|g| self.tokens[g.unwrap()].raw.start),
        );
        // An innermost run of scopes holding only their own controls before
        // the selection is dropped, instead of closing it as an empty group.
        let only_controls = |from: usize| {
            !self
                .units
                .iter()
                .any(|u| u.raw.start >= from && u.raw.start < start)
                && self
                    .tokens
                    .iter()
                    .filter(|t| t.raw.start >= from && t.raw.end <= start)
                    .all(|t| match t.kind {
                        Kind::Open => true,
                        Kind::Control(c) => character_control(c) && !bases.contains(&t.raw),
                        _ => false,
                    })
        };
        let trimmed = closing.iter().position(|&from| only_controls(from));
        let closes = trimmed.unwrap_or(closing.len());
        let cut = trimmed.map_or(start, |n| closing[n]);
        let wrap = root_first.is_some() && trimmed != Some(0);
        // Likewise a scope re-opened only to be closed again at once.
        let mut reopen = scopes.len() - 1 - keep;
        let mut tail = end;
        for token in self.tokens.iter().filter(|t| t.raw.end > end) {
            match token.kind {
                _ if reopen == 0 => break,
                Kind::Close => {
                    reopen -= 1;
                    tail = token.raw.end;
                }
                Kind::Control(c) if character_control(c) => {}
                _ => break,
            }
        }
        let mut out = "}".repeat(closes);
        out.push_str(&middle);
        for control in &scopes[0].controls {
            out.push_str(&self.raw[control.clone()]);
        }
        for scope in &scopes[1 + keep..1 + keep + reopen] {
            out.push('{');
            for control in &scope.controls {
                out.push_str(&self.raw[control.clone()]);
            }
        }
        // A re-emitted `\C` could absorb a following number as its ACI suffix.
        if out.ends_with(';') && self.raw[tail..].starts_with(|c: char| c.is_ascii_digit()) {
            out.push_str("{}");
        }
        let mut patches = vec![(cut..tail, out)];
        if let Some(at) = root_first.filter(|_| wrap) {
            patches.push((at..at, "{".into()));
        }
        apply_patches(self.raw, patches)
    }

    /// Every unselected cell and every paragraph property stays identical, and
    /// each selected cell carries exactly the object default afterwards.
    fn certified_clear(
        &self,
        raw: String,
        range: &Range<usize>,
        default: &SpanProperties,
    ) -> Result<String, EditError> {
        self.certify(&raw, self.visible_len.unwrap())?;
        let old_doc = super::raw_edit::editor_document(self.raw);
        let new_doc = super::raw_edit::editor_document(&raw);
        if old_doc.paragraphs.first().map(|p| &p.properties)
            != new_doc.paragraphs.first().map(|p| &p.properties)
        {
            return Err(EditError::ParagraphBoundary);
        }
        let before = doc_to_cells(&old_doc);
        let after = doc_to_cells(&new_doc);
        if before.len() != after.len() {
            return Err(EditError::ProjectionMismatch);
        }
        for (n, (old, new)) in before.iter().zip(&after).enumerate() {
            let selected = range.contains(&n);
            let same = match (old, new) {
                (Cell::Char(a, _), Cell::Char(b, p)) if selected => a == b && p == default,
                (
                    Cell::Stack {
                        data: a, head: h, ..
                    },
                    Cell::Stack {
                        data: b,
                        props,
                        head: k,
                    },
                ) if selected => a == b && h == k && props == default,
                _ => old == new,
            };
            if !same {
                return Err(EditError::UnsupportedInput);
            }
        }
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn clear(raw: &str, range: Range<usize>) -> Result<String, EditError> {
        SourceIndex::new(raw).cleared_selection(range, &[])
    }
    /// With the leading font and width kept, as the object defaults will be.
    fn clear_keeping_base(raw: &str, range: Range<usize>) -> Result<String, EditError> {
        let index = SourceIndex::new(raw);
        let keep: Vec<_> = index
            .tokens
            .iter()
            .take_while(|t| t.kind != Kind::Text)
            .filter(|t| matches!(t.kind, Kind::Control('f' | 'W')))
            .map(|t| t.raw.clone())
            .collect();
        index.cleared_selection(range, &keep)
    }
    #[test]
    fn clear_keeps_paragraph_tabs_fraction_and_object_defaults_verbatim() {
        let raw = r"\fArial;\W1.5;{\C2;\Léa}b\pxqc,t4;\P\S1/2;";
        assert_eq!(
            clear_keeping_base(raw, 0..2).unwrap(),
            r"\fArial;\W1.5;{éa}b\pxqc,t4;\P\S1/2;"
        );
    }
    #[test]
    fn partial_inherited_group_scope_is_split_with_verbatim_controls() {
        for (raw, range, expected) in [
            (r"{\C1;abc}", 1..2, r"{\C1;a}b{\C1;c}"),
            (r"{\C1;\H2x;abc}", 0..1, r"a{\C1;\H2x;bc}"),
            (r"{\Lab}", 1..2, r"{\La}b"),
            (r"{{\Labc}}", 1..2, r"{{\La}b{\Lc}}"),
            (r"{\C1;ab}{\Lcd}", 1..3, r"{\C1;a}bc{\Ld}"),
            (r"{\C1;a{\Lbc}d}", 1..2, r"{\C1;a}b{\C1;{\Lc}d}"),
            (
                r"{\c255;a\U+00D8{\Lb}c}",
                1..3,
                r"{\c255;a}\U+00D8{}b{\c255;c}",
            ),
        ] {
            assert_eq!(clear(raw, range).unwrap(), expected, "{raw:?}");
        }
    }
    #[test]
    fn root_level_controls_are_wrapped_and_re_emitted_after_the_selection() {
        for (raw, range, expected) in [
            (r"x\C1;abc", 2..3, r"x{\C1;a}b\C1;c"),
            (r"\C1;abc", 0..1, r"a\C1;bc"),
            (r"ab\C1;cd", 1..3, r"abc\C1;d"),
            (r"\C1;a\P\pqc;b\Pc", 2..3, r"{\C1;a\P\pqc;}b\C1;\Pc"),
        ] {
            assert_eq!(clear(raw, range).unwrap(), expected, "{raw:?}");
        }
    }
    #[test]
    fn split_never_changes_unselected_cells_or_paragraph_properties() {
        for (raw, range) in [
            (r"\pxqc,t4;{\C1;\Lab\Pcd}ef", 1..4),
            (r"{\H0.5x;a\S1/2;b}c", 1..4),
            (r"\Q15;\T1.2;\A1;a{\O\K b}c\o", 1..3),
        ] {
            let index = SourceIndex::new(raw);
            let out = clear(raw, range.clone()).unwrap();
            let before = doc_to_cells(&super::super::raw_edit::editor_document(raw));
            let after = doc_to_cells(&super::super::raw_edit::editor_document(&out));
            for (n, (a, b)) in before.iter().zip(&after).enumerate() {
                if !range.contains(&n) {
                    assert_eq!(a, b, "{raw:?} cell {n}");
                }
            }
            assert_eq!(index.raw, raw);
        }
    }
    #[test]
    fn every_editable_range_certifies_and_a_second_clear_is_a_no_op() {
        for raw in [
            r"\fArial|b0|i0;\W1.2;x\C1;a{\H2x;b{\Lc\U+00D8d}e}f\Pg{\Oh}",
            r"\pxqc,t4;{\C1;\Lab\Pcd}ef\S1/2;{\Kgh}",
            r"{\c255;{}{\Q12;ab}c}\T1.5;de{\A1;f\P}g",
            r"{\La}{\Lb}{{\C2;c}}",
        ] {
            let index = SourceIndex::new(raw);
            let n = index.visible_len.unwrap();
            let before = doc_to_cells(&super::super::raw_edit::editor_document(raw));
            for a in 0..n {
                for b in a + 1..=n {
                    if index.validate_range(&(a..b)).is_err() {
                        continue;
                    }
                    let out = clear(raw, a..b).unwrap();
                    let after = doc_to_cells(&super::super::raw_edit::editor_document(&out));
                    assert_eq!(before.len(), after.len(), "{raw:?} {a}..{b}");
                    for (n, (x, y)) in before.iter().zip(&after).enumerate() {
                        if !(a..b).contains(&n) {
                            assert_eq!(x, y, "{raw:?} {a}..{b} -> {out:?}");
                        }
                    }
                    assert!(SourceIndex::new(&out).protected.iter().all(|p| matches!(
                        p.reason,
                        super::super::raw_adapter::Protection::Stack
                            | super::super::raw_adapter::Protection::PreserveOnly
                    )));
                    assert_eq!(clear(&out, a..b).as_ref(), Ok(&out), "{out:?} {a}..{b}");
                }
            }
        }
    }
    #[test]
    fn partial_inherited_scope_unknown_fields_and_no_selection_are_refused() {
        for (raw, range) in [
            (r"{\pqc;\Lab}", 1..2),
            (r"{\L\pqc;ab}c", 1..3),
            (r"a\Zopaque", 0..1),
            (r"%<field>%", 0..1),
            (r"{\La}", 0..0),
            (r"{\La\Nb}", 0..3),
        ] {
            assert!(clear(raw, range).is_err(), "{raw:?}");
        }
    }
    #[test]
    fn kept_defaults_stay_and_a_bold_kept_font_is_refused() {
        assert_eq!(
            clear_keeping_base(r"\fArial;\W1.5;\C1;ab\Lc", 1..2).unwrap(),
            r"\fArial;\W1.5;{\C1;a}b\C1;\Lc"
        );
        // MTEXT stores bold only in a named font: a kept bold font leaves no
        // code that could mean "not bold".
        assert!(clear_keeping_base(r"\fArial|b1;{\Lab}", 1..2).is_err());
        assert_eq!(
            clear_keeping_base(r"\fArial|b0|i0|c238|p34;{\Lab}", 1..2).unwrap(),
            r"\fArial|b0|i0|c238|p34;{\La}b"
        );
    }
}
