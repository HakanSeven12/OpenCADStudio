//! Source patches only. Every untouched raw slice is copied verbatim.
use super::raw_adapter::{EditError, Kind, SourceIndex};
use std::ops::Range;

pub(super) struct TextEdit {
    pub raw: String,
    pub caret: usize,
}

impl SourceIndex<'_> {
    pub fn replace(&self, range: Range<usize>, plain: &str) -> Result<TextEdit, EditError> {
        self.validate_range(&range)?;
        let (mut inserted, added) = encode_plain(plain)?;
        let selected: Vec<_> = self
            .units
            .iter()
            .filter(|u| u.visible.start < range.end && u.visible.end > range.start)
            .collect();
        for unit in &selected {
            if unit.visible.start < range.start
                || unit.visible.end > range.end
                || unit.kind == Kind::Column
            {
                return Err(EditError::AtomicStructure);
            }
        }
        let mut at = selected
            .first()
            .map(|u| u.raw.start)
            .unwrap_or(self.insertion_byte(range.start)?);
        if plain.contains(['\r', '\n']) {
            let next_break = self
                .tokens
                .iter()
                .find(|t| matches!(t.kind, Kind::Break | Kind::Column) && t.raw.start >= at);
            let paragraph_end = next_break.map_or(self.raw.len(), |t| t.raw.start);
            let late_settings = self.tokens.iter().any(|t| {
                t.kind == Kind::Control('p') && t.raw.start >= at && t.raw.start < paragraph_end
            });
            if late_settings {
                let logical_end = next_break
                    .and_then(|t| t.visible.as_ref())
                    .map_or(self.visible_len.unwrap(), |v| v.start);
                let target = next_break.map_or(self.tail_boundary(), |t| t.raw.start);
                let only_settings = self
                    .tokens
                    .iter()
                    .filter(|t| t.raw.start >= at && t.raw.end <= target)
                    .all(|t| t.kind == Kind::Control('p'));
                // An end-of-paragraph newline goes AFTER its trailing settings.
                // Elsewhere the paragraph repair below restores the settings.
                if range.is_empty() && range.start == logical_end && only_settings {
                    at = target;
                }
            }
        }
        let legacy_boundary = self.units.iter().any(|u| {
            u.raw.end == at
                && (u.kind == Kind::Unicode
                    || (u.kind == Kind::Escape && &self.raw[u.raw.clone()] == r"\\"))
        });
        if added > 0 && legacy_boundary && !inserted.starts_with("{}") {
            inserted.insert_str(0, "{}");
        }
        let mut patches: Vec<_> = selected
            .iter()
            .map(|u| (u.raw.clone(), String::new()))
            .collect();
        let paragraph_edit = self.paragraph_edit(&range, &selected, plain);
        if let Some(edit) = &paragraph_edit {
            // Settings of paragraphs merged away go with them.
            patches.extend(edit.dropped.iter().map(|r| (r.clone(), String::new())));
        }
        if let Some(first) = patches.first_mut() {
            first.1 = inserted;
        } else if !inserted.is_empty() {
            patches.push((at..at, inserted));
        }
        let expected = self.visible_len.unwrap() - (range.end - range.start) + added;
        let mut raw = apply_patches(self.raw, patches.clone())?;
        if SourceIndex::new(&raw).visible_len != Some(expected) {
            // A numeric/hex/caret adjacency can extend an existing control.
            // Re-plan ONLY the insertion with an inert empty-group separator;
            // no source token or complete document is ever reserialized.
            if added > 0 {
                if let Some(first) = patches.first_mut() {
                    first.1.insert_str(0, "{}");
                    raw = apply_patches(self.raw, patches)?;
                }
            }
        }
        if let Some(edit) = paragraph_edit {
            raw = edit.repair(&raw)?;
        }
        self.certify(&raw, expected)?;
        Ok(TextEdit {
            raw,
            caret: range.start + added,
        })
    }

    /// Paragraph extents (raw byte ranges) and the `\\p` controls of each.
    fn paragraphs(&self) -> Vec<(Range<usize>, Vec<Range<usize>>)> {
        let mut out = Vec::new();
        let mut start = 0;
        let mut settings = Vec::new();
        for token in &self.tokens {
            match token.kind {
                Kind::Break | Kind::Column => {
                    out.push((start..token.raw.start, std::mem::take(&mut settings)));
                    start = token.raw.end;
                }
                Kind::Control('p') => settings.push(token.raw.clone()),
                _ => {}
            }
        }
        out.push((start..self.raw.len(), settings));
        out
    }

    /// Plan how paragraph settings follow a text replacement, when any exist:
    /// every paragraph produced from the edited one keeps its settings, the
    /// settings of paragraphs merged into it are dropped, the rest stay.
    fn paragraph_edit(
        &self,
        range: &Range<usize>,
        selected: &[&super::raw_adapter::Unit],
        plain: &str,
    ) -> Option<ParagraphEdit> {
        if !self.tokens.iter().any(|t| t.kind == Kind::Control('p')) {
            return None;
        }
        let first = self
            .units
            .iter()
            .filter(|u| {
                matches!(u.kind, Kind::Break | Kind::Column) && u.visible.start < range.start
            })
            .count();
        let removed = selected.iter().filter(|u| u.kind == Kind::Break).count();
        let added = plain
            .replace("\r\n", "\n")
            .chars()
            .filter(|c| matches!(c, '\r' | '\n'))
            .count();
        // Text inside one paragraph cannot change any paragraph's settings.
        if removed == 0 && added == 0 {
            return None;
        }
        let paragraphs = self.paragraphs();
        let before: Vec<_> = super::raw_edit::editor_document(self.raw)
            .paragraphs
            .into_iter()
            .map(|p| p.properties)
            .collect();
        if before.len() != paragraphs.len() {
            return None;
        }
        let mut expected = before[..first].to_vec();
        expected.extend(std::iter::repeat_n(before[first].clone(), added + 1));
        expected.extend_from_slice(&before[first + removed + 1..]);
        Some(ParagraphEdit {
            first,
            settings: paragraphs[first]
                .1
                .iter()
                .map(|r| self.raw[r.clone()].to_string())
                .collect(),
            dropped: paragraphs[first + 1..=first + removed]
                .iter()
                .flat_map(|(_, settings)| settings.iter().cloned())
                .collect(),
            produced: added + 1,
            expected,
        })
    }

    /// Format source units, never controls outside the selection. Adjacent
    /// units needing the same code share one new group. A group that holds
    /// nothing but its own character controls and exactly the selected units
    /// is updated in place instead: the controls the new code supersedes are
    /// removed and the code is added, so repeated toggling never nests deeper.
    pub fn format(
        &self,
        range: Range<usize>,
        mut prefix: impl FnMut(usize) -> Result<String, EditError>,
    ) -> Result<String, EditError> {
        self.validate_range(&range)?;
        let groups = self.enclosing_groups();
        // (first unit, last unit, code, enclosing group open token)
        let mut runs: Vec<(usize, usize, String, Option<usize>)> = Vec::new();
        for (n, unit) in self.units.iter().enumerate() {
            if unit.visible.start >= range.end || unit.visible.end <= range.start {
                continue;
            }
            if matches!(unit.kind, Kind::Stack | Kind::Column) {
                return Err(EditError::AtomicStructure);
            }
            if unit.kind == Kind::Break {
                continue;
            }
            let code = prefix(unit.visible.start)?;
            if code.is_empty() {
                continue;
            }
            match runs.last_mut() {
                Some((_, last, previous, group))
                    if *previous == code
                        && *group == groups[n]
                        && self.units[*last].raw.end == unit.raw.start =>
                {
                    *last = n
                }
                _ => runs.push((n, n, code, groups[n])),
            }
        }
        let mut patches = Vec::new();
        for (first, last, code, group) in runs {
            let span = self.units[first].raw.start..self.units[last].raw.end;
            if let Some(controls) = group.and_then(|open| self.owned_group_controls(open, &span)) {
                let classes = control_classes(&code);
                for control in controls {
                    if classes.contains(&control_class(&self.raw[control.clone()])) {
                        patches.push((control, String::new()));
                    }
                }
                patches.push((span.start..span.start, code));
                continue;
            }
            let tail = &self.units[last];
            let legacy = tail.kind == Kind::Unicode
                || (tail.kind == Kind::Escape && &self.raw[tail.raw.clone()] == r"\\");
            patches.push((
                span.clone(),
                format!(
                    "{{{code}{}{}}}",
                    &self.raw[span],
                    if legacy { "{}" } else { "" }
                ),
            ));
        }
        let raw = apply_patches(self.raw, patches)?;
        self.certify(&raw, self.visible_len.unwrap())?;
        Ok(raw)
    }

    /// The innermost open-group token around each unit.
    fn enclosing_groups(&self) -> Vec<Option<usize>> {
        let mut stack = Vec::new();
        let mut out = Vec::with_capacity(self.units.len());
        let mut next = 0;
        for (i, token) in self.tokens.iter().enumerate() {
            match token.kind {
                Kind::Open => stack.push(i),
                Kind::Close => {
                    stack.pop();
                }
                _ => {}
            }
            while self
                .units
                .get(next)
                .is_some_and(|u| u.raw.end <= token.raw.end)
            {
                out.push(stack.last().copied());
                next += 1;
            }
        }
        out
    }

    /// The group's own controls when it contains only character controls
    /// followed by exactly the units in `span`; otherwise `None`.
    fn owned_group_controls(&self, open: usize, span: &Range<usize>) -> Option<Vec<Range<usize>>> {
        let close = self.tokens[open].partner?;
        let mut controls = Vec::new();
        for token in &self.tokens[open + 1..close] {
            match token.kind {
                Kind::Control(c) if is_character_control(c) && token.raw.end <= span.start => {
                    controls.push(token.raw.clone())
                }
                Kind::Text | Kind::Escape | Kind::Unicode | Kind::Percent
                    if token.raw.start >= span.start && token.raw.end <= span.end => {}
                _ => return None,
            }
        }
        // The span must reach both ends of the group's units.
        let units_start = self.tokens[open + 1..close]
            .iter()
            .find(|t| !matches!(t.kind, Kind::Control(_)))?
            .raw
            .start;
        let units_end = self.tokens[close].raw.start;
        (units_start == span.start && units_end == span.end).then_some(controls)
    }

    pub fn align_paragraphs(&self, range: Range<usize>, align: char) -> Result<String, EditError> {
        self.validate_range(&range)?;
        if !matches!(align, 'l' | 'c' | 'r' | 'j') {
            return Err(EditError::InvalidRange);
        }
        let paragraph_at = |pos: usize| {
            self.units
                .iter()
                .filter(|u| matches!(u.kind, Kind::Break | Kind::Column) && u.visible.start < pos)
                .count()
        };
        let first = paragraph_at(range.start);
        let last = paragraph_at(if range.end > range.start {
            range.end - 1
        } else {
            range.start
        });
        let mut ends: Vec<usize> = self
            .tokens
            .iter()
            .filter(|t| matches!(t.kind, Kind::Break | Kind::Column))
            .map(|t| t.raw.start)
            .collect();
        ends.push(self.tail_boundary());
        let patches = ends
            .into_iter()
            .enumerate()
            .filter(|(n, _)| *n >= first && *n <= last)
            .map(|(_, end)| (end..end, format!("\\pq{align};")))
            .collect();
        let raw = apply_patches(self.raw, patches)?;
        self.certify(&raw, self.visible_len.unwrap())?;
        Ok(raw)
    }

    pub(super) fn insertion_byte(&self, caret: usize) -> Result<usize, EditError> {
        // Left affinity inside a paragraph; after a break use the next run's
        // leading controls. No byte position inside an escape or a fraction.
        if let Some(previous) = self.units.iter().find(|u| u.visible.end == caret) {
            if !matches!(previous.kind, Kind::Break | Kind::Column) {
                let mut byte = previous.raw.end;
                // An inert delimiter belongs before following input, not after
                // it: do not let left-affinity typing rejoin a literal \\U+ code.
                while self.raw[byte..].starts_with("{}") {
                    byte += 2;
                }
                return Ok(byte);
            }
        }
        if let Some(next) = self.units.iter().find(|u| u.visible.start == caret) {
            return Ok(next.raw.start);
        }
        if caret == self.visible_len.unwrap_or(usize::MAX) {
            return Ok(self.tail_boundary());
        }
        Err(EditError::InvalidRange)
    }

    pub fn tail_boundary(&self) -> usize {
        let mut end = self.raw.len();
        for token in self.tokens.iter().rev() {
            if token.kind != Kind::Close {
                break;
            }
            end = token.raw.start;
        }
        end
    }

    pub fn max_group_depth(&self) -> usize {
        let (mut depth, mut maximum) = (0usize, 0usize);
        for token in &self.tokens {
            match token.kind {
                Kind::Open => {
                    depth += 1;
                    maximum = maximum.max(depth);
                }
                Kind::Close => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        maximum
    }

    pub(super) fn certify(&self, raw: &str, expected: usize) -> Result<(), EditError> {
        let candidate = SourceIndex::new(raw);
        if candidate.visible_len != Some(expected) {
            return Err(EditError::ProjectionMismatch);
        }
        // Conservative adapter policy: preserve already imported deeper groups,
        // but never raise the maximum beyond the existing depth or eight.
        if candidate.max_group_depth() > self.max_group_depth().max(8) {
            return Err(EditError::NestingLimit);
        }
        Ok(())
    }
}

struct ParagraphEdit {
    first: usize,
    settings: Vec<String>,
    dropped: Vec<Range<usize>>,
    produced: usize,
    expected: Vec<codec::entities::mtext_format::ParagraphProperties>,
}

impl ParagraphEdit {
    /// Re-assert the edited paragraph's own settings at the end of each
    /// produced paragraph that lost them (a later `\\p` wins in its
    /// paragraph), then certify every paragraph's properties.
    fn repair(&self, raw: &str) -> Result<String, EditError> {
        let actual = |raw: &str| -> Vec<_> {
            super::raw_edit::editor_document(raw)
                .paragraphs
                .into_iter()
                .map(|p| p.properties)
                .collect()
        };
        let current = actual(raw);
        if current == self.expected {
            return Ok(raw.to_string());
        }
        if current.len() != self.expected.len() {
            return Err(EditError::ParagraphBoundary);
        }
        let index = SourceIndex::new(raw);
        let ends: Vec<usize> = index
            .tokens
            .iter()
            .filter(|t| matches!(t.kind, Kind::Break | Kind::Column))
            .map(|t| t.raw.start)
            .chain(std::iter::once(index.tail_boundary()))
            .collect();
        let copy = self.settings.concat();
        let patches = (self.first..self.first + self.produced)
            .filter(|&n| current[n] != self.expected[n])
            .map(|n| (ends[n]..ends[n], copy.clone()))
            .collect();
        let repaired = apply_patches(raw, patches)?;
        if actual(&repaired) != self.expected {
            return Err(EditError::ParagraphBoundary);
        }
        Ok(repaired)
    }
}

pub(super) fn is_character_control(code: char) -> bool {
    matches!(
        code,
        'f' | 'F' | 'H' | 'W' | 'C' | 'c' | 'Q' | 'T' | 'A' | 'L' | 'l' | 'O' | 'o' | 'K' | 'k'
    )
}

/// Controls of one class set the same property: a later one replaces it.
fn control_class(control: &str) -> char {
    match control.chars().nth(1).unwrap_or('?') {
        'F' => 'f',
        'l' => 'L',
        'o' => 'O',
        'k' => 'K',
        'c' => 'C',
        other => other,
    }
}

fn control_classes(code: &str) -> Vec<char> {
    SourceIndex::new(code)
        .tokens
        .iter()
        .filter(|t| matches!(t.kind, Kind::Control(_)))
        .map(|t| control_class(&code[t.raw.clone()]))
        .collect()
}

pub(super) fn apply_patches(
    raw: &str,
    mut patches: Vec<(Range<usize>, String)>,
) -> Result<String, EditError> {
    patches.sort_by_key(|(r, _)| (r.start, r.end));
    let mut result = String::new();
    let mut previous = 0;
    for (range, replacement) in patches {
        if range.start < previous
            || range.start > range.end
            || !raw.is_char_boundary(range.start)
            || !raw.is_char_boundary(range.end)
        {
            return Err(EditError::InvalidRange);
        }
        result.push_str(&raw[previous..range.start]);
        result.push_str(&replacement);
        previous = range.end;
    }
    result.push_str(&raw[previous..]);
    Ok(result)
}

/// Plain clipboard/text input cannot introduce MTEXT, FIELD, percent, or caret
/// control codes. Only the inserted newline sequence is normalized.
fn encode_plain(plain: &str) -> Result<(String, usize), EditError> {
    let normalized = plain.replace("\r\n", "\n").replace('\r', "\n");
    if normalized
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(EditError::UnsupportedInput);
    }
    if normalized.is_empty() {
        return Ok((String::new(), 0));
    }
    // Do not wrap each keystroke in a group: left-affinity typing would nest
    // those groups indefinitely. Empty groups delimit new Unicode escapes.
    let mut raw = String::new();
    for ch in normalized.chars() {
        match ch {
            '\\' => raw.push_str(r"{}\\{}"),
            '{' => raw.push_str("\\{"),
            '}' => raw.push_str("\\}"),
            '\n' => raw.push_str("\\P"),
            '%' => raw.push_str("{}\\U+0025{}"),
            '^' => raw.push_str("{}\\U+005E{}"),
            _ => raw.push(ch),
        }
    }
    Ok((raw, normalized.chars().count()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::mtext_format::parse_mtext;
    #[test]
    fn newline_cannot_move_late_paragraph_settings_off_untouched_prefix_text() {
        let raw = r"\pxqc,t4;ab\pqr;cd";
        let index = SourceIndex::new(raw);
        assert_eq!(
            index.replace(1..1, "\n").err(),
            Some(EditError::ParagraphBoundary)
        );
        assert_eq!(index.raw, raw);
    }
    #[test]
    fn end_of_paragraph_newline_retains_its_late_alignment_and_tabs() {
        for raw in [r"\pxqc,t4;ab\pqr;", r"\pxqc,t4;ab\pqr;\Pcd"] {
            let edit = SourceIndex::new(raw).replace(2..2, "\n").unwrap();
            let doc = parse_mtext(&edit.raw, true);
            assert_eq!(
                doc.paragraphs[0].properties.alignment,
                Some(codec::entities::mtext_format::MTextParagraphAlignment::Right)
            );
            assert_eq!(doc.paragraphs[0].properties.tab_stops.len(), 1);
            assert!(edit.raw.starts_with(r"\pxqc,t4;ab\pqr;\P"));
        }
    }
    #[test]
    fn repeated_plain_and_escape_insertion_has_bounded_depth() {
        let mut raw = String::new();
        let mut expected = String::new();
        for ch in "abé%d^I".chars().cycle().take(512) {
            let index = SourceIndex::new(&raw);
            let end = index.visible_len.unwrap();
            let edit = index.replace(end..end, &ch.to_string()).unwrap();
            raw = edit.raw;
            expected.push(ch);
            assert!(SourceIndex::new(&raw).max_group_depth() <= 1);
        }
        assert_eq!(parse_mtext(&raw, true).to_plain_text(), expected);
    }
    #[test]
    fn depth_guard_rejects_additional_scopes_but_allows_plain_text_patch() {
        // A group holding only the target is updated in place, no deeper.
        let owned = format!("{}x{}", "{".repeat(8), "}".repeat(8));
        let out = SourceIndex::new(&owned)
            .format(0..1, |_| Ok("\\L".into()))
            .unwrap();
        assert_eq!(out, format!("{}\\Lx{}", "{".repeat(8), "}".repeat(8)));
        // A group shared with unselected text must be wrapped: refused at 8.
        let raw = format!("{}xy{}", "{".repeat(8), "}".repeat(8));
        let index = SourceIndex::new(&raw);
        assert_eq!(
            index.format(0..1, |_| Ok("\\L".into())).err(),
            Some(EditError::NestingLimit)
        );
        assert_eq!(
            index.replace(0..0, "z").unwrap().raw,
            format!("{}zxy{}", "{".repeat(8), "}".repeat(8))
        );
        assert_eq!(index.raw, raw);
    }
    #[test]
    fn unicode_replacement_only_changes_target_payload() {
        let raw = r"\pxqc,t4;{\fArial|b1;Árvíz} \C1;piros";
        let e = SourceIndex::new(raw).replace(1..4, "ő😀").unwrap();
        assert_eq!(e.raw, r"\pxqc,t4;{\fArial|b1;Áő😀z} \C1;piros");
        assert_eq!(e.caret, 3);
        assert_eq!(parse_mtext(&e.raw, true).to_plain_text(), "Áő😀z piros");
    }
    #[test]
    fn replacement_across_nested_runs_keeps_all_control_bytes() {
        let raw = r"{\Labc\l}{\C1;def}ghi";
        let e = SourceIndex::new(raw).replace(1..8, "X").unwrap();
        assert_eq!(e.raw, r"{\LaX\l}{\C1;}i");
        assert_eq!(parse_mtext(&e.raw, true).to_plain_text(), "aXi");
    }
    #[test]
    fn newline_and_join_are_safe_without_paragraph_settings() {
        let e = SourceIndex::new(r"a\Pb").replace(1..2, "").unwrap();
        assert_eq!(e.raw, "ab");
        let e = SourceIndex::new("ab").replace(1..1, "\r\n").unwrap();
        assert_eq!(e.raw, r"a\Pb");
        assert_eq!(e.caret, 2);
    }
    #[test]
    fn paragraph_merge_keeps_the_first_paragraph_settings() {
        fn props(raw: &str) -> Vec<codec::entities::mtext_format::ParagraphProperties> {
            super::super::raw_edit::editor_document(raw)
                .paragraphs
                .into_iter()
                .map(|p| p.properties)
                .collect()
        }
        for (raw, range, expected) in [
            // The merged paragraph is the first one: its tabs stay.
            (r"\pxqc,t4;a\Pb", 1..2, r"\pxqc,t4;ab"),
            // The second paragraph's own settings go with its break.
            (r"a\P\pqr;b", 1..2, "ab"),
            (r"\pqc;a\P\pxt8;b\Pc", 1..2, r"\pqc;ab\Pc"),
            // A selection spanning two breaks merges three paragraphs.
            (r"\pqc;ab\P\pqr;cd\P\pql;ef", 1..7, r"\pqc;af"),
        ] {
            let before = props(raw);
            let e = SourceIndex::new(raw).replace(range, "").unwrap();
            assert_eq!(e.raw, expected, "{raw:?}");
            assert_eq!(props(&e.raw)[0], before[0], "{raw:?}");
        }
        let e = SourceIndex::new(r"\pxqc,t4;a\Pb")
            .replace(3..3, "é")
            .unwrap();
        assert!(e.raw.starts_with(r"\pxqc,t4;"));
    }
    #[test]
    fn splitting_a_paragraph_gives_both_halves_its_settings() {
        for (raw, at) in [
            // Settings written at the end, as the alignment button does.
            (r"abcd\pqc;\Pef", 2),
            // Settings at the start, as imported drawings usually have.
            (r"\pxi1,qr,t4;abcd\Pef", 2),
            // Both, the later one winning.
            (r"\pqc;ab\pqr;cd", 1),
        ] {
            let paragraphs = |raw: &str| {
                super::super::raw_edit::editor_document(raw)
                    .paragraphs
                    .into_iter()
                    .map(|p| p.properties)
                    .collect::<Vec<_>>()
            };
            let before = paragraphs(raw);
            let e = SourceIndex::new(raw).replace(at..at, "\n").unwrap();
            let after = paragraphs(&e.raw);
            assert_eq!(after.len(), before.len() + 1, "{raw:?}");
            assert_eq!(after[0], before[0], "{raw:?} -> {:?}", e.raw);
            assert_eq!(after[1], before[0], "{raw:?} -> {:?}", e.raw);
            assert_eq!(after[2..], before[1..], "{raw:?}");
            assert_eq!(
                parse_mtext(&e.raw, true).to_plain_text().replace('\n', ""),
                parse_mtext(raw, true).to_plain_text().replace('\n', "")
            );
        }
    }
    #[test]
    fn clipboard_codes_and_fields_are_literal() {
        let plain = "\\C1; {} %%d ^I %<\\AcVar X>% e\u{301}";
        let e = SourceIndex::new("").replace(0..0, plain).unwrap();
        assert_eq!(parse_mtext(&e.raw, true).to_plain_text(), plain);
        assert_eq!(
            SourceIndex::new(&e.raw).visible_len,
            Some(plain.chars().count())
        );
    }
    #[test]
    fn columns_and_fraction_interiors_are_protected() {
        let i = SourceIndex::new(r"a\S1/2;b\Nc");
        assert_eq!(i.replace(2..3, "x").err(), Some(EditError::InvalidRange));
        assert_eq!(i.replace(5..6, "").err(), Some(EditError::AtomicStructure));
        assert_eq!(i.replace(1..4, "x").unwrap().raw, r"axb\Nc");
    }
    #[test]
    fn unsupported_input_and_source_never_use_a_fallback() {
        assert_eq!(
            SourceIndex::new(r"a\Zpayload").replace(0..1, "x").err(),
            Some(EditError::UnknownStructure)
        );
        assert_eq!(
            SourceIndex::new("a").replace(0..1, "\0").err(),
            Some(EditError::UnsupportedInput)
        );
    }
    #[test]
    fn scoped_formatting_keeps_outer_codes_and_other_characters() {
        let raw = r"\L{\C1;abc}\l";
        let out = SourceIndex::new(raw)
            .format(1..2, |_| Ok("\\l".into()))
            .unwrap();
        assert_eq!(out, r"\L{\C1;a{\lb}c}\l");
        let cells = super::super::doc_to_cells(&parse_mtext(&out, true));
        assert!(matches!(&cells[0], super::super::Cell::Char(_, p) if p.underline()));
        assert!(matches!(&cells[1], super::super::Cell::Char(_, p) if !p.underline()));
        assert!(matches!(&cells[2], super::super::Cell::Char(_, p) if p.underline()));
    }
    #[test]
    fn alignment_appends_only_alignment_and_observes_selection_end() {
        let raw = r"\pxqc,i-1,l2,t4;ab\P\pqr;cd";
        let out = SourceIndex::new(raw).align_paragraphs(0..3, 'l').unwrap();
        assert_eq!(out, r"\pxqc,i-1,l2,t4;ab\pql;\P\pqr;cd");
        let doc = parse_mtext(&out, true);
        assert_eq!(doc.paragraphs[0].properties.tab_stops.len(), 1);
        assert_eq!(
            doc.paragraphs[1].properties.alignment,
            Some(codec::entities::mtext_format::MTextParagraphAlignment::Right)
        );
    }
}
