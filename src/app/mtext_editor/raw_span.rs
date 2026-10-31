//! Character and paragraph values for the selection, by source patches: the
//! selected characters get one code, touched paragraphs one setting, and a
//! patch is kept only when exactly that value changed and nothing else.
use super::raw_adapter::{EditError, SourceIndex};
use super::raw_edit::editor_document;
use super::raw_operations::{apply_patches, TextEdit};
use super::*;
use std::ops::Range;

/// The properties `code` gives a character on its own.
fn template(code: &str) -> SpanProperties {
    parse_mtext(&format!("{code}x"), true)
        .paragraphs
        .first()
        .and_then(|p| p.spans.first())
        .map(|s| s.properties.clone())
        .unwrap_or_default()
}

/// Put the code `code_for` gives each selected character in front of it.
/// `copy` moves the one value the code sets from a template; each character
/// must come out as it was with only that value changed.
fn patch_span(
    raw: &str,
    range: Range<usize>,
    code_for: impl Fn(&SpanProperties) -> Result<String, EditError>,
    copy: fn(&mut SpanProperties, &SpanProperties),
) -> Result<String, EditError> {
    let index = SourceIndex::new(raw);
    let cells = doc_to_cells(&editor_document(raw));
    let expected = |p: &SpanProperties| -> Result<SpanProperties, EditError> {
        let mut want = p.clone();
        copy(&mut want, &template(&code_for(p)?));
        Ok(want)
    };
    let out = index.format(range.clone(), |n| match cells.get(n) {
        Some(Cell::Char(_, p)) if expected(p)? == *p => Ok(String::new()),
        Some(Cell::Char(_, p)) => code_for(p),
        _ => Err(EditError::ProjectionMismatch),
    })?;
    let after = doc_to_cells(&editor_document(&out));
    let mut certified = after.len() == cells.len();
    for (n, (old, new)) in cells.iter().zip(&after).enumerate() {
        certified &= match (old, new) {
            (Cell::Char(a, p), Cell::Char(b, q)) if range.contains(&n) => {
                a == b && expected(p)? == *q
            }
            (old, new) => old == new,
        };
    }
    if !certified {
        return Err(EditError::ProjectionMismatch);
    }
    Ok(out)
}

/// The paragraphs a visible `range` touches (the caret's own when empty).
fn touched_paragraphs(index: &SourceIndex, range: &Range<usize>) -> Range<usize> {
    let paragraph_at = |pos: usize| {
        index
            .units
            .iter()
            .filter(|u| {
                matches!(
                    u.kind,
                    super::raw_adapter::Kind::Break | super::raw_adapter::Kind::Column
                ) && u.visible.start < pos
            })
            .count()
    };
    let last = if range.end > range.start {
        range.end - 1
    } else {
        range.start
    };
    paragraph_at(range.start)..paragraph_at(last) + 1
}

/// Add the paragraph setting `code` at the end of every paragraph `range`
/// touches. `copy` moves the one value it sets; each touched paragraph must
/// come out with only that value changed, the others and the text as they
/// were.
fn patch_paragraphs(
    raw: &str,
    range: Range<usize>,
    code: &str,
    copy: fn(&mut ParagraphProperties, &ParagraphProperties),
) -> Result<String, EditError> {
    let index = SourceIndex::new(raw);
    let count = index.validate(false)?;
    index.validate_range(&range)?;
    let touched = touched_paragraphs(&index, &range);
    let mut ends: Vec<usize> = index
        .tokens
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                super::raw_adapter::Kind::Break | super::raw_adapter::Kind::Column
            )
        })
        .map(|t| t.raw.start)
        .collect();
    ends.push(index.tail_boundary());
    let patches = ends
        .into_iter()
        .enumerate()
        .filter(|(n, _)| touched.contains(n))
        .map(|(_, end)| (end..end, code.to_string()))
        .collect();
    let out = apply_patches(raw, patches)?;
    index.certify(&out, count)?;
    let setting = parse_mtext(&format!("{code}x"), true)
        .paragraphs
        .first()
        .map(|p| p.properties.clone())
        .unwrap_or_default();
    let before = editor_document(raw).paragraphs;
    let after = editor_document(&out).paragraphs;
    let certified = before.len() == after.len()
        && before
            .iter()
            .zip(&after)
            .enumerate()
            .all(|(n, (old, new))| {
                let mut want = old.properties.clone();
                if touched.contains(&n) {
                    copy(&mut want, &setting);
                }
                old.spans == new.spans && want == new.properties
            });
    if !certified {
        return Err(EditError::ParagraphBoundary);
    }
    Ok(out)
}

/// A number as a code writes it.
pub(super) fn number(value: f64) -> String {
    let text = format!("{value:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl crate::app::OpenCADStudio {
    /// Height, oblique angle, width factor or tracking of the selection.
    pub(in crate::app) fn raw_span_number(&mut self, field: &str, value: f64) {
        let range = match self.mtext_editor.as_ref() {
            Some(ed) => ed.selected_range(),
            None => return,
        };
        let code = match field {
            "height" => format!("\\H{};", number(value)),
            "oblique" => format!("\\Q{};", number(value)),
            "width" => format!("\\W{};", number(value)),
            "tracking" => format!("\\T{};", number(value)),
            _ => return,
        };
        let copy: fn(&mut SpanProperties, &SpanProperties) = match field {
            "height" => |p, t| p.height = t.height.clone(),
            "oblique" => |p, t| p.oblique_angle = t.oblique_angle,
            "width" => |p, t| p.width_factor = t.width_factor,
            _ => |p, t| p.tracking = t.tracking,
        };
        self.raw_selection_patch(range, |raw, range| {
            patch_span(raw, range, |_| Ok(code.clone()), copy)
        });
    }

    /// The font of the selection; each character keeps its bold and italic.
    pub(in crate::app) fn raw_selection_font(&mut self, name: &str) {
        let range = match self.mtext_editor.as_ref() {
            Some(ed) => ed.selected_range(),
            None => return,
        };
        self.raw_selection_patch(range, |raw, range| {
            patch_span(
                raw,
                range,
                |p| {
                    let mut font = p.font.clone().unwrap_or_else(|| MTextFont::from_name(name));
                    font.name = name.to_owned();
                    super::raw_edit::font_code(&font)
                },
                |p, t| p.font = t.font.clone(),
            )
        });
    }

    /// The colour of the selection: an index, a true colour, ByLayer or
    /// ByBlock, as the MTEXT colour codes store them.
    pub(in crate::app) fn raw_selection_color(&mut self, color: codec::types::Color) {
        use codec::types::Color as C;
        let code = match color {
            C::Index(i) => format!("\\C{i};"),
            C::Rgb { r, g, b } => format!(
                "\\c{};",
                u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
            ),
            C::ByBlock => "\\C0;".into(),
            _ => "\\C256;".into(),
        };
        let range = match self.mtext_editor.as_ref() {
            Some(ed) => ed.selected_range(),
            None => return,
        };
        self.raw_selection_patch(range, |raw, range| {
            patch_span(
                raw,
                range,
                |_| Ok(code.clone()),
                |p, t| {
                    p.color = t.color.clone();
                    p.color_rgb = t.color_rgb;
                },
            )
        });
    }

    /// Alignment of every paragraph the selection (or caret) touches.
    pub(in crate::app) fn raw_align(&mut self, align: ParaAlign) {
        let code = match align {
            ParaAlign::Left => "\\pql;",
            ParaAlign::Center => "\\pqc;",
            ParaAlign::Right => "\\pqr;",
            ParaAlign::Justify => "\\pqj;",
        };
        self.raw_paragraph(code, |p, s| p.alignment = s.alignment);
    }

    /// An indent or paragraph spacing of every paragraph the selection (or
    /// caret) touches.
    pub(in crate::app) fn raw_paragraph_number(&mut self, field: ParaNumber, value: f64) {
        let (key, copy): (char, fn(&mut ParagraphProperties, &ParagraphProperties)) = match field {
            ParaNumber::FirstIndent => ('i', |p, s| p.first_line_indent = s.first_line_indent),
            ParaNumber::LeftIndent => ('l', |p, s| p.left_margin = s.left_margin),
            ParaNumber::RightIndent => ('r', |p, s| p.right_margin = s.right_margin),
            ParaNumber::SpaceBefore => ('b', |p, s| p.spacing_before = s.spacing_before),
            ParaNumber::SpaceAfter => ('a', |p, s| p.spacing_after = s.spacing_after),
        };
        self.raw_paragraph(&format!("\\px{key}{};", number(value)), copy);
    }

    fn raw_paragraph(
        &mut self,
        code: &str,
        copy: fn(&mut ParagraphProperties, &ParagraphProperties),
    ) {
        let range = match self.mtext_editor.as_ref() {
            Some(ed) => ed.selected_range(),
            None => return,
        };
        self.raw_selection_patch(range, |raw, range| patch_paragraphs(raw, range, code, copy));
    }

    /// Replace every occurrence of the find text, as one undo step. Each
    /// replacement is an ordinary text edit of that place only.
    pub(in crate::app) fn raw_replace_all(&mut self) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let needle: Vec<char> = ed.find_text.chars().collect();
        if needle.is_empty() {
            return;
        }
        let cells = doc_to_cells(&ed.doc);
        let mut found = Vec::new();
        let mut at = 0;
        while at + needle.len() <= cells.len() {
            let hit = cells[at..at + needle.len()]
                .iter()
                .zip(&needle)
                .all(|(cell, want)| matches!(cell, Cell::Char(c, _) if c == want));
            if hit {
                found.push(at);
                at += needle.len();
            } else {
                at += 1;
            }
        }
        if found.is_empty() {
            return;
        }
        let replacement = ed.replace_text.clone();
        self.edit_raw(|_, index| {
            let mut raw = index.raw.to_string();
            let mut caret = 0;
            // From the last match back, so earlier positions stay valid.
            for (n, start) in found.iter().rev().enumerate() {
                let edit =
                    SourceIndex::new(&raw).replace(*start..start + needle.len(), &replacement)?;
                if n == 0 {
                    caret = edit.caret;
                }
                raw = edit.raw;
            }
            let shift = (found.len() - 1) * replacement.chars().count();
            let shift_back = (found.len() - 1) * needle.len();
            Ok(TextEdit {
                raw,
                caret: caret + shift - shift_back,
            })
        });
    }

    /// One selection patch as one undo step; the selection stays.
    fn raw_selection_patch(
        &mut self,
        range: Range<usize>,
        patch: impl FnOnce(&str, Range<usize>) -> Result<String, EditError>,
    ) {
        self.edit_raw(|_, index| {
            Ok(TextEdit {
                raw: patch(index.raw, range.clone())?,
                caret: range.end,
            })
        });
        self.keep_selection(range);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::mtext_editor::{Cell, ParaAlign, ParaNumber};
    use crate::app::OpenCADStudio;
    use codec::entities::mtext_format::{MTextColor, MTextParagraphAlignment, SpanProperties};
    use glam::DVec3;

    fn app(raw: &str) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs
            .push(crate::app::document::DocumentTab::new_drawing(1));
        app.active_tab = 1;
        app.open_mtext_editor(DVec3::ZERO, None, raw, 2.5, None);
        app
    }
    fn raw(app: &OpenCADStudio) -> String {
        app.mtext_editor.as_ref().unwrap().content.text()
    }
    fn select(app: &mut OpenCADStudio, a: usize, b: usize) {
        let ed = app.mtext_editor.as_mut().unwrap();
        ed.sel = Some((a, b));
        ed.sel_anchor = a;
        ed.caret = b;
    }
    fn props(app: &OpenCADStudio, n: usize) -> SpanProperties {
        match &super::doc_to_cells(&app.mtext_editor.as_ref().unwrap().doc)[n] {
            Cell::Char(_, p) => p.clone(),
            _ => panic!("char"),
        }
    }

    #[test]
    fn a_selection_value_patches_only_the_selection_and_keeps_every_code() {
        let mut app = app(r"\pxqc,t4;ab{\Lcd}");
        select(&mut app, 1, 3);
        app.mtext_apply_span_number("height", "5".into());
        assert_eq!(raw(&app), r"\pxqc,t4;a{\H5;b}{\L{\H5;c}d}");
        assert_eq!(app.mtext_editor.as_ref().unwrap().sel, Some((1, 3)));
        for (field, value) in [("oblique", "10"), ("width", "1.5"), ("tracking", "1.2")] {
            app.mtext_apply_span_number(field, value.into());
        }
        let p = props(&app, 2);
        assert_eq!(p.oblique_angle, Some(10.0));
        assert_eq!(p.width_factor, Some(1.5));
        assert_eq!(p.tracking, Some(1.2));
        assert!(p.underline());
        assert_eq!(props(&app, 0), SpanProperties::default());
        // Each value was one step.
        for _ in 0..4 {
            app.mtext_undo();
        }
        assert_eq!(raw(&app), r"\pxqc,t4;ab{\Lcd}");
    }

    #[test]
    fn a_selection_font_keeps_each_character_bold_or_not() {
        let mut app = app(r"{\fArial|b1;ab}c");
        select(&mut app, 1, 3);
        app.mtext_apply_font("Times New Roman");
        let b = props(&app, 1).font.unwrap();
        assert_eq!((b.name.as_str(), b.bold), ("Times New Roman", true));
        let c = props(&app, 2).font.unwrap();
        assert_eq!((c.name.as_str(), c.bold), ("Times New Roman", false));
        assert_eq!(props(&app, 0).font.unwrap().name, "Arial");
    }

    #[test]
    fn a_selection_colour_is_an_index_a_true_colour_or_by_layer() {
        let mut app = app("abc");
        select(&mut app, 0, 1);
        app.mtext_apply_color(codec::types::Color::Index(3));
        select(&mut app, 1, 2);
        app.mtext_apply_color(codec::types::Color::Rgb {
            r: 10,
            g: 20,
            b: 30,
        });
        assert_eq!(props(&app, 0).color, Some(MTextColor::Index(3)));
        assert_eq!(props(&app, 1).color_rgb, Some((10, 20, 30)));
        select(&mut app, 0, 1);
        app.mtext_apply_color(codec::types::Color::ByLayer);
        assert_eq!(props(&app, 0).color, Some(MTextColor::ByLayer));
        // The object colour was not touched by any of them.
        assert_eq!(app.mtext_editor.as_ref().unwrap().object_color, None);
    }

    #[test]
    fn alignment_and_indents_change_only_the_touched_paragraphs() {
        let mut app = app(r"\pxt4;ab\Pcd\Pef");
        select(&mut app, 3, 4);
        app.mtext_apply_align(ParaAlign::Center);
        app.mtext_apply_paragraph_number(ParaNumber::FirstIndent, "2".into());
        let doc = &app.mtext_editor.as_ref().unwrap().doc;
        let props: Vec<_> = doc.paragraphs.iter().map(|p| &p.properties).collect();
        assert_eq!(props[1].alignment, Some(MTextParagraphAlignment::Center));
        assert_eq!(props[1].first_line_indent, Some(2.0));
        assert_eq!(props[0].alignment, None);
        assert_eq!(props[2].first_line_indent, None);
        assert_eq!(doc.to_plain_text(), "ab\ncd\nef");
        assert!(raw(&app).starts_with(r"\pxt4;ab\P"));
    }

    #[test]
    fn replace_all_rewrites_only_the_matches_as_one_step() {
        let mut app = app(r"\pxqc,t4;a{\Lxa}a");
        let ed = app.mtext_editor.as_mut().unwrap();
        ed.find_text = "a".into();
        ed.replace_text = "bb".into();
        app.mtext_replace_all();
        assert_eq!(raw(&app), r"\pxqc,t4;bb{\Lxbb}bb");
        assert!(props(&app, 4).underline());
        app.mtext_undo();
        assert_eq!(raw(&app), r"\pxqc,t4;a{\Lxa}a");
    }

    #[test]
    fn a_protected_text_is_left_as_it_was() {
        let mut app = app(r"ab\Zopaque");
        select(&mut app, 0, 2);
        app.mtext_apply_span_number("height", "5".into());
        app.mtext_apply_color(codec::types::Color::Index(1));
        app.mtext_apply_align(ParaAlign::Right);
        assert_eq!(raw(&app), r"ab\Zopaque");
    }

    #[test]
    fn alignment_appends_only_alignment_and_observes_the_selection_end() {
        // The selection ends on the break: only the first paragraph changes,
        // and its tab stops and the second one's alignment stay.
        let mut app = app(r"\pxqc,i-1,l2,t4;ab\P\pqr;cd");
        select(&mut app, 0, 3);
        app.mtext_apply_align(ParaAlign::Left);
        assert_eq!(raw(&app), r"\pxqc,i-1,l2,t4;ab\pql;\P\pqr;cd");
        let doc = &app.mtext_editor.as_ref().unwrap().doc;
        assert_eq!(doc.paragraphs[0].properties.tab_stops.len(), 1);
        assert_eq!(
            doc.paragraphs[1].properties.alignment,
            Some(MTextParagraphAlignment::Right)
        );
    }
}
