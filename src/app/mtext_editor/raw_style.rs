//! Object defaults kept in the MTEXT source itself: the object font is the
//! `\f`/`\F` and the object width the `\W` that open the text, ahead of any
//! character. Changing one patches only those controls (and the runs that
//! merely repeat them); every other byte stays.
use super::raw_adapter::{EditError, Kind, SourceIndex};
use super::raw_edit::editor_document;
use super::raw_operations::apply_patches;
use super::*;
use std::ops::Range;

/// What the text style supplies when the source has no object default.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::app) struct ObjectStyle {
    /// The family the style draws with, as bold/italic runs name it.
    pub face: String,
    /// The style's own width factor.
    pub width: f64,
    /// Bold/italic the style's TrueType face supplies, i.e. what a run
    /// without its own `|b`/`|i` flags shows once the object font is reset.
    pub font_flags: (bool, bool),
}

/// The `code` controls ahead of the first character that apply to the whole
/// text: the object defaults. That is a top-level control, or one inside
/// groups that hold all of the text, as AutoCAD writes a whole-text font
/// (`{\fArial|b0|i0;...}`). A control in a group that ends before the last
/// character, or after any character, formats only part of it.
pub(super) fn leading_controls(raw: &str, code: char) -> Vec<Range<usize>> {
    let index = SourceIndex::new(raw);
    let end = index.units.first().map_or(raw.len(), |u| u.raw.start);
    let text_end = index.units.last().map_or(raw.len(), |u| u.raw.end);
    // Where each group closes, by the position of its opening brace.
    let mut closes = std::collections::HashMap::new();
    let mut open = Vec::new();
    for t in &index.tokens {
        match t.kind {
            Kind::Open => open.push(t.raw.start),
            Kind::Close => {
                if let Some(start) = open.pop() {
                    closes.insert(start, t.raw.start);
                }
            }
            _ => {}
        }
    }
    let mut enclosing: Vec<usize> = Vec::new();
    let mut ranges = Vec::new();
    for t in &index.tokens {
        if t.raw.start >= end {
            break;
        }
        match t.kind {
            Kind::Open => enclosing.push(t.raw.start),
            Kind::Close => {
                enclosing.pop();
            }
            Kind::Control(c)
                if c.eq_ignore_ascii_case(&code)
                    && enclosing
                        .iter()
                        .all(|g| closes.get(g).is_some_and(|close| *close >= text_end)) =>
            {
                ranges.push(t.raw.clone())
            }
            _ => {}
        }
    }
    ranges
}

fn width_code(raw: &str, range: &Range<usize>) -> Result<f64, EditError> {
    raw[range.start + 2..range.end - 1]
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.)
        .ok_or(EditError::ObjectDefault)
}

fn text_style<'a>(doc: &'a codec::Document, name: &str) -> Option<&'a codec::tables::TextStyle> {
    doc.text_styles.iter().find(|s| {
        s.name.eq_ignore_ascii_case(name)
            || (name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
    })
}

impl ObjectStyle {
    pub(super) fn of(doc: &codec::Document, name: &str) -> Self {
        use crate::entities::text_support::{TTF_FLAG_BOLD, TTF_FLAG_ITALIC};
        let style = text_style(doc, name);
        let font = crate::entities::text_support::resolve_text_style(name, doc).font_name;
        Self {
            face: crate::scene::text::sysfont::split_style(&font)
                .0
                .to_string(),
            width: style
                .map(|s| s.width_factor)
                .filter(|w| w.is_finite() && *w > 0.)
                .unwrap_or(1.),
            font_flags: style
                .filter(|s| !s.true_type_font.trim().is_empty())
                .map_or((false, false), |s| {
                    let flags = s.true_type_font_flags;
                    (flags & TTF_FLAG_BOLD != 0, flags & TTF_FLAG_ITALIC != 0)
                }),
        }
    }
}

/// The width the text draws with: the style's width times the object `\W`.
fn base_width(raw: &str, style: f64) -> Result<f64, EditError> {
    let factor = leading_controls(raw, 'W')
        .last()
        .map_or(Ok(1.), |r| width_code(raw, r))?;
    Ok(style * factor)
}

/// Set the object width to `target` (the style's own with `None`). Every
/// other `\W` keeps its ratio to the object width, so the text scales as
/// one; the object `\W` is written relative to the style's width.
fn patch_width(raw: &str, style: f64, target: Option<f64>) -> Result<String, EditError> {
    let index = SourceIndex::new(raw);
    let count = index.validate(false)?;
    let leading = leading_controls(raw, 'W');
    let old_base = base_width(raw, style)?;
    let wanted = target.unwrap_or(style);
    let ratio = wanted / old_base;
    let relative = wanted / style;
    if ![wanted, ratio, relative]
        .iter()
        .all(|v| v.is_finite() && *v > 0.)
    {
        return Err(EditError::ObjectDefault);
    }
    let mut patches = Vec::new();
    for t in &index.tokens {
        if matches!(t.kind, Kind::Control('W')) {
            let value = width_code(raw, &t.raw)? * ratio;
            if !value.is_finite() || value <= 0. {
                return Err(EditError::ObjectDefault);
            }
            if leading.contains(&t.raw) {
                patches.push((t.raw.clone(), String::new()));
            } else if ratio != 1. {
                patches.push((t.raw.clone(), format!("\\W{value};")));
            }
        }
    }
    if target.is_some() {
        patches.push((0..0, format!("\\W{relative};")));
    }
    let out = apply_patches(raw, patches)?;
    index.certify(&out, count)?;
    Ok(out)
}

fn font_of(code: &str) -> Option<MTextFont> {
    parse_mtext(&format!("{code}x"), true)
        .paragraphs
        .first()?
        .spans
        .first()?
        .properties
        .font
        .clone()
}

/// The object font: the last font control ahead of the first character.
fn root_font(raw: &str) -> Option<MTextFont> {
    let last = leading_controls(raw, 'f').last()?.clone();
    font_of(&raw[last])
}

/// The one font every character in `cells` is written in, when they share
/// one; a character in the style's font, or two faces, gives none.
fn uniform_font(cells: &[Cell]) -> Option<MTextFont> {
    let mut font: Option<MTextFont> = None;
    for cell in cells {
        let (Cell::Char(_, p) | Cell::Stack { props: p, .. }) = cell else {
            continue;
        };
        let own = p.font.as_ref().filter(|f| !f.name.trim().is_empty())?;
        match &font {
            None => font = Some(own.clone()),
            Some(f) if f.name.eq_ignore_ascii_case(&own.name) => {}
            Some(_) => return None,
        }
    }
    font
}

/// The font the whole text is written in: the object font, or else the one
/// font every character shares, however the codes group them (AutoCAD can
/// write a font group per paragraph, and text typed later can sit outside
/// the first one).
fn text_font(raw: &str) -> Option<MTextFont> {
    root_font(raw).or_else(|| uniform_font(&doc_to_cells(&editor_document(raw))))
}

/// Runs whose own font only repeats the object face (bold and italic must
/// name a face to be stored) follow an object font change, keeping their
/// flags. Runs in a genuinely different face keep it.
fn follow_object_font(raw: &str, old: &str, new: &str) -> Result<String, EditError> {
    if old.is_empty() || old.eq_ignore_ascii_case(new) {
        return Ok(raw.to_owned());
    }
    let index = SourceIndex::new(raw);
    let count = index.validate(false)?;
    let leading = leading_controls(raw, 'f');
    let mut patches = Vec::new();
    for t in &index.tokens {
        if !matches!(t.kind, Kind::Control('f' | 'F')) || leading.contains(&t.raw) {
            continue;
        }
        let Some(mut font) = font_of(&raw[t.raw.clone()]) else {
            continue;
        };
        if !font.name.eq_ignore_ascii_case(old) {
            continue;
        }
        font.name = new.to_owned();
        patches.push((t.raw.clone(), super::raw_edit::font_code(&font)?));
    }
    let out = apply_patches(raw, patches)?;
    index.certify(&out, count)?;
    Ok(out)
}

/// Set the object font to `name`, or back to the style's with `None`. The
/// object font keeps its own flags. MTEXT stores bold/italic only inside a
/// named font, so it goes back to the style only where every explicit flag
/// already equals what the style gives; an unknown font field is never
/// dropped.
fn patch_font(
    raw: &str,
    name: Option<&str>,
    style_flags: (bool, bool),
) -> Result<String, EditError> {
    let index = SourceIndex::new(raw);
    let count = index.validate(false)?;
    let old = root_font(raw);
    // A new object font keeps the flags of the font the text is written in.
    let template = old.clone().or_else(|| text_font(raw));
    if name.is_none()
        && old.as_ref().is_some_and(|f| {
            (f.bold_explicit && f.bold != style_flags.0)
                || (f.italic_explicit && f.italic != style_flags.1)
                || !f.extra.is_empty()
                || !f.style.is_empty()
        })
    {
        return Err(EditError::ObjectDefault);
    }
    let mut patches: Vec<_> = leading_controls(raw, 'f')
        .into_iter()
        .map(|r| (r, String::new()))
        .collect();
    if let Some(name) = name {
        let mut font = template.unwrap_or_else(|| MTextFont::from_name(name));
        font.name = name.to_owned();
        patches.push((0..0, super::raw_edit::font_code(&font)?));
    }
    let out = apply_patches(raw, patches)?;
    index.certify(&out, count)?;
    Ok(out)
}

/// The object font and width controls, which clearing a character's
/// formatting leaves in place.
pub(super) fn object_controls(raw: &str) -> Vec<Range<usize>> {
    let mut controls = leading_controls(raw, 'f');
    controls.extend(leading_controls(raw, 'W'));
    controls
}

/// Where a first-row value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum ValueOrigin {
    Style,
    Custom,
    /// From the style, but some characters override it.
    Mixed,
}

impl MTextEditorState {
    /// Fill the font and width fields and the style's defaults from the
    /// source and the current style.
    pub(super) fn seed_object_values(&mut self, doc: &codec::Document) {
        let raw = self.content.text();
        self.object_style = ObjectStyle::of(doc, &self.style);
        self.font = text_font(&raw).map_or_else(String::new, |f| f.name);
        self.width = number_text(base_width(&raw, self.object_style.width).unwrap_or(1.));
    }

    /// What the Font field shows: the selection's font when every selected
    /// character shares one, else the font of the whole text.
    pub(in crate::app) fn font_field(&self) -> String {
        let range = self.selected_range();
        if !range.is_empty() {
            let selected = doc_to_cells(&self.doc).get(range).and_then(uniform_font);
            if let Some(font) = selected {
                return font.name;
            }
        }
        self.font.clone()
    }

    pub(in crate::app) fn value_origin(&self, field: &str) -> ValueOrigin {
        let raw = self.content.text();
        let code = if field == "font" { 'f' } else { 'W' };
        if !leading_controls(&raw, code).is_empty() || field == "font" && text_font(&raw).is_some()
        {
            return ValueOrigin::Custom;
        }
        // A run that only repeats the style's face to carry bold or italic
        // does not make the font mixed.
        let overridden = SourceIndex::new(&raw).tokens.iter().any(|t| match t.kind {
            Kind::Control('f' | 'F') if field == "font" => font_of(&raw[t.raw.clone()])
                .is_none_or(|f| !f.name.eq_ignore_ascii_case(&self.object_style.face)),
            Kind::Control('W') => field == "width",
            _ => false,
        });
        if overridden {
            ValueOrigin::Mixed
        } else {
            ValueOrigin::Style
        }
    }
}

/// A number as a field shows it: no trailing zeros.
fn number_text(value: f64) -> String {
    let text = format!("{value:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl crate::app::OpenCADStudio {
    /// Put an object-default change into the source as one undo step; a
    /// refused change leaves everything as it was and says why.
    fn set_object_source(&mut self, result: Result<String, EditError>) -> bool {
        let unlocated = self.unlocated_field_host();
        let Some(ed) = self.mtext_editor.as_mut() else {
            return false;
        };
        let old = ed.content.text();
        // The fields of a FIELD host must stay where they are.
        let result = result.and_then(|raw| {
            if unlocated && raw != old {
                return Err(EditError::NativeField);
            }
            let fields = ed.track_fields(&old, &raw, None)?;
            Ok((raw, fields))
        });
        match result {
            Ok((raw, fields)) => {
                if raw != old {
                    ed.record_undo();
                    ed.content = text_editor::Content::with_text(&raw);
                    if fields.is_some() {
                        ed.field_host = fields;
                    }
                }
                true
            }
            Err(error) => {
                self.command_line
                    .push_error(crate::t!(error.to_string()).as_ref());
                false
            }
        }
    }

    /// The width field with nothing selected: the object width. The field
    /// keeps what is typed; a valid number goes into the source.
    pub(in crate::app) fn mtext_object_width(&mut self, value: String) {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        let number = value
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v > 0.);
        let raw = ed.content.text();
        let style = ed.object_style.width;
        ed.width = value;
        if let Some(number) = number {
            if base_width(&raw, style).ok() != Some(number) {
                self.set_object_source(patch_width(&raw, style, Some(number)));
            }
        }
        self.rebuild_mtext_preview();
    }

    /// The font menu with nothing selected: the object font; `[Style default]`
    /// goes back to the style's.
    pub(in crate::app) fn mtext_object_font(&mut self, font: &str) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let raw = ed.content.text();
        let name = (font != "[Style default]").then_some(font);
        let face = &ed.object_style.face;
        let old = text_font(&raw).map_or_else(|| face.clone(), |f| f.name);
        let new = name.map_or_else(|| face.clone(), str::to_owned);
        let result = patch_font(&raw, name, ed.object_style.font_flags)
            .and_then(|raw| follow_object_font(&raw, &old, &new));
        if self.set_object_source(result) {
            if let Some(ed) = self.mtext_editor.as_mut() {
                ed.font = name.unwrap_or_default().to_owned();
            }
        }
        self.rebuild_mtext_preview();
    }

    /// A new text style keeps the width the text draws with: the object `\W`
    /// is rewritten against the new style's width.
    pub(in crate::app) fn mtext_object_style(&mut self, name: String) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let raw = ed.content.text();
        let doc = &self.tabs[self.active_tab].scene.document;
        let style = ObjectStyle::of(doc, &name);
        let result = if leading_controls(&raw, 'W').is_empty() {
            Ok(raw.clone())
        } else {
            base_width(&raw, ed.object_style.width)
                .and_then(|target| patch_width(&raw, style.width, Some(target)))
        };
        if let (Ok(_), Some(ed)) = (&result, self.mtext_editor.as_mut()) {
            ed.record_undo();
        }
        if self.set_object_source(result) {
            if let Some(ed) = self.mtext_editor.as_mut() {
                ed.style = name;
                ed.object_style = style;
                ed.width = number_text(
                    base_width(&ed.content.text(), ed.object_style.width).unwrap_or(1.),
                );
            }
        }
        self.rebuild_mtext_preview();
    }
}

#[cfg(test)]
mod tests {
    use super::ValueOrigin;
    use crate::app::OpenCADStudio;
    use glam::DVec3;

    fn app(raw: &str) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs
            .push(crate::app::document::DocumentTab::new_drawing(1));
        app.active_tab = 1;
        let doc = &mut app.tabs[1].scene.document;
        let mut wide = codec::tables::TextStyle::new("Wide");
        wide.handle = doc.allocate_handle();
        wide.width_factor = 2.;
        doc.text_styles.add(wide).unwrap();
        app.open_mtext_editor(DVec3::ZERO, None, raw, 2.5, None);
        app
    }
    fn raw(app: &OpenCADStudio) -> String {
        app.mtext_editor.as_ref().unwrap().content.text()
    }
    fn ed(app: &OpenCADStudio) -> &crate::app::mtext_editor::MTextEditorState {
        app.mtext_editor.as_ref().unwrap()
    }

    #[test]
    fn opening_shows_the_object_font_and_width_and_where_they_come_from() {
        let app = app(r"\fArial;\W1.5;a{\W2;b}");
        assert_eq!(ed(&app).font, "Arial");
        assert_eq!(ed(&app).width, "1.5");
        assert_eq!(ed(&app).value_origin("font"), ValueOrigin::Custom);
        assert_eq!(ed(&app).value_origin("width"), ValueOrigin::Custom);
        let app = self::app(r"a{\W2;b}");
        assert_eq!(ed(&app).font, "");
        assert_eq!(ed(&app).width, "1");
        assert_eq!(ed(&app).value_origin("font"), ValueOrigin::Style);
        assert_eq!(ed(&app).value_origin("width"), ValueOrigin::Mixed);
    }

    #[test]
    fn the_object_font_patches_only_its_own_controls() {
        // A bold run that only repeats the object face follows it; a run in
        // its own face and the tab stops stay byte for byte.
        let mut app = app(r"\fArial;\pxqc,t4;a{\fArial|b1;b}{\fCourier;c}");
        app.mtext_apply_font("Simplex");
        assert_eq!(
            raw(&app),
            r"\fSimplex;\pxqc,t4;a{\fSimplex|b1;b}{\fCourier;c}"
        );
        assert_eq!(ed(&app).font, "Simplex");
        // One undo step brings the whole change back.
        app.mtext_undo();
        assert_eq!(raw(&app), r"\fArial;\pxqc,t4;a{\fArial|b1;b}{\fCourier;c}");
    }

    #[test]
    fn style_default_removes_the_object_font_unless_it_carries_bold() {
        let mut app = app(r"\fArial;ab");
        app.mtext_apply_font("[Style default]");
        assert_eq!(raw(&app), "ab");
        assert_eq!(ed(&app).font, "");
        // Bold lives only in the named font: dropping it would unbold the text.
        let mut app = self::app(r"\fArial|b1;ab");
        app.mtext_apply_font("[Style default]");
        assert_eq!(raw(&app), r"\fArial|b1;ab");
        assert_eq!(ed(&app).font, "Arial");
    }

    #[test]
    fn the_object_width_scales_every_width_code_as_one() {
        let mut app = app(r"ab{\W2;c}");
        app.mtext_apply_span_number("width", "1.5".into());
        assert_eq!(raw(&app), r"\W1.5;ab{\W3;c}");
        assert_eq!(ed(&app).width, "1.5");
        // Half-typed input is shown but changes nothing.
        app.mtext_apply_span_number("width", "".into());
        assert_eq!(raw(&app), r"\W1.5;ab{\W3;c}");
        assert_eq!(ed(&app).width, "");
    }

    #[test]
    fn a_new_style_keeps_the_width_the_text_draws_with() {
        let mut app = app(r"\W1.5;ab");
        app.mtext_object_style("Wide".into());
        // 1.5 wide on a style twice as wide is a 0.75 object factor.
        assert_eq!(raw(&app), r"\W0.75;ab");
        assert_eq!(ed(&app).style, "Wide");
        assert_eq!(ed(&app).width, "1.5");
        // Without an object width the text takes the new style's own.
        let mut app = self::app("ab");
        app.mtext_object_style("Wide".into());
        assert_eq!(raw(&app), "ab");
        assert_eq!(ed(&app).width, "2");
        assert_eq!(ed(&app).value_origin("width"), ValueOrigin::Style);
    }

    #[test]
    fn clearing_the_whole_text_keeps_the_object_font_and_width() {
        let mut app = app(r"\fArial;\W1.5;{\C1;ab}c");
        app.mtext_clear_formatting();
        assert_eq!(raw(&app), r"\fArial;\W1.5;{ab}c");
    }

    #[test]
    fn the_saved_value_is_the_source_with_the_object_values() {
        let mut app = app("ab");
        app.mtext_apply_font("Arial");
        app.mtext_apply_span_number("width", "2".into());
        assert_eq!(ed(&app).build_mtext().value, r"\W2;\fArial;ab");
    }

    #[test]
    fn an_autocad_whole_text_font_group_is_the_object_font() {
        // AutoCAD wraps a whole-text font in one group around all the text.
        let mut app = app(r"{\fISOCTEUR|b0|i0|c238|p34;asd\Pqwe}");
        assert_eq!(ed(&app).font, "ISOCTEUR");
        assert_eq!(ed(&app).value_origin("font"), ValueOrigin::Custom);
        app.mtext_apply_font("Arial");
        assert_eq!(raw(&app), r"\fArial|b0|i0|c238|p34;{asd\Pqwe}");
        assert_eq!(ed(&app).font, "Arial");
        // A group around part of the text formats only that part.
        let app = self::app(r"{\fISOCTEUR;ab}c");
        assert_eq!(ed(&app).font, "");
        assert_eq!(ed(&app).value_origin("font"), ValueOrigin::Mixed);
    }

    #[test]
    fn a_font_every_character_shares_is_changed_whatever_groups_hold_it() {
        // AutoCAD may write a font group per paragraph or per run: the text
        // is still in one font.
        for source in [
            r"{\fISOCTEUR|b0|i0|c238|p34;asd}\P{\fISOCTEUR|b0|i0|c238|p34;qwe}",
            r"{\fISOCTEUR|b0|i0|c238|p34;asd}{\fISOCTEUR|b0|i0|c238|p34;\S2^;}",
        ] {
            let mut app = app(source);
            assert_eq!(ed(&app).font, "ISOCTEUR", "{source}");
            assert_eq!(ed(&app).value_origin("font"), ValueOrigin::Custom);
            app.mtext_apply_font("Arial");
            assert_eq!(ed(&app).font, "Arial", "{source}");
            let cells = super::doc_to_cells(&ed(&app).doc);
            for cell in &cells {
                if let super::Cell::Char(_, p) | super::Cell::Stack { props: p, .. } = cell {
                    let font = p.font.as_ref().unwrap();
                    assert_eq!(font.name, "Arial", "{source}: {}", raw(&app));
                    assert_eq!(font.charset, Some(238), "{source}: flags kept");
                }
            }
        }
        // Characters in the style's font beside one in another: no one font.
        let app = self::app(r"a{\fISOCTEUR;b}");
        assert_eq!(ed(&app).font, "");
    }

    #[test]
    fn the_font_field_shows_the_selections_own_font() {
        let mut app = app(r"a{\fCourier New;bc}");
        let select = |app: &mut OpenCADStudio, a: usize, b: usize| {
            let ed = app.mtext_editor.as_mut().unwrap();
            ed.sel = Some((a, b));
            ed.sel_anchor = a;
            ed.caret = b;
        };
        select(&mut app, 1, 3);
        assert_eq!(ed(&app).font_field(), "Courier New");
        // After a font for the selection, the field names it.
        app.mtext_apply_font("Arial");
        assert_eq!(ed(&app).font_field(), "Arial");
        // A selection in two fonts shows the text's own (here the style's).
        select(&mut app, 0, 3);
        assert_eq!(ed(&app).font_field(), "");
    }
}
