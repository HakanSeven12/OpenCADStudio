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
    /// The style's font as the font picker names it.
    pub font: Option<crate::ui::font_picker::FontChoice>,
    /// The style's own width factor.
    pub width: f64,
    /// The style's oblique angle, in degrees.
    pub oblique: f64,
    /// The style's fixed height; 0 when the height is asked for each text.
    pub height: f64,
    /// Bold/italic the style's TrueType face supplies, i.e. what a run
    /// without its own `|b`/`|i` flags shows once the object font is reset.
    pub font_flags: (bool, bool),
    /// An external reference's style, which cannot be changed here.
    pub read_only: bool,
}

/// Whether `raw` sets `code` (`\W`, `\Q`, …) for all of its text, so the
/// text has its own value instead of the style's.
pub(in crate::app) fn sets_whole_text(raw: &str, code: char) -> bool {
    !leading_controls(raw, code).is_empty()
}

/// The `code` controls ahead of the first character that apply to the whole
/// text: the object defaults. That is a top-level control, or one inside
/// groups that hold all of the text, as other writers store a whole-text font
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
    // `\W<value>;`: a code cut short (no `;`, no value) names no width.
    raw.get(range.start + 2..range.end.saturating_sub(1))
        .ok_or(EditError::ObjectDefault)?
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
            font: style.and_then(crate::ui::font_picker::style_font),
            width: style
                .map(|s| s.width_factor)
                .filter(|w| w.is_finite() && *w > 0.)
                .unwrap_or(1.),
            oblique: style
                .map(|s| s.oblique_angle.to_degrees())
                .filter(|o| o.is_finite())
                .unwrap_or(0.),
            height: style
                .map(|s| s.height)
                .filter(|h| h.is_finite() && *h > 0.)
                .unwrap_or(0.),
            font_flags: style
                .filter(|s| !s.true_type_font.trim().is_empty())
                .map_or((false, false), |s| {
                    let flags = s.true_type_font_flags;
                    (flags & TTF_FLAG_BOLD != 0, flags & TTF_FLAG_ITALIC != 0)
                }),
            read_only: style.is_some_and(|s| s.xref_dependent || s.name.contains('|')),
        }
    }
}

/// The width the text draws with: the style's width times the object `\W`.
pub(super) fn base_width(raw: &str, style: f64) -> Result<f64, EditError> {
    let factor = leading_controls(raw, 'W')
        .last()
        .map_or(Ok(1.), |r| width_code(raw, r))?;
    Ok(style * factor)
}

/// Set the object width to `target` (the style's own with `None`). Every
/// other `\W` keeps its ratio to the object width, so the text scales as
/// one; the object `\W` is written relative to the style's width.
pub(super) fn patch_width(raw: &str, style: f64, target: Option<f64>) -> Result<String, EditError> {
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
pub(super) fn root_font(raw: &str) -> Option<MTextFont> {
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
/// font every character shares, however the codes group them (other writers can
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
pub(super) fn patch_font(
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

/// The object font, width, oblique, tracking and colour controls, which
/// clearing a character's formatting leaves in place.
pub(super) fn object_controls(raw: &str) -> Vec<Range<usize>> {
    let mut controls = leading_controls(raw, 'f');
    for code in ['W', 'Q', 'T', 'C'] {
        controls.extend(leading_controls(raw, code));
    }
    controls
}

/// The value of the whole-text `\Q` or `\T`; `None` without one.
pub(super) fn object_number(raw: &str, code: char) -> Option<f64> {
    leading_controls(raw, code)
        .last()
        .and_then(|r| raw.get(r.start + 2..r.end.saturating_sub(1)))
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

/// Replace the whole-text `code` control with `control` (`None` removes it).
/// A code that formats only part of the text keeps overriding it.
pub(super) fn patch_object_control(
    raw: &str,
    code: char,
    control: Option<String>,
) -> Result<String, EditError> {
    let index = SourceIndex::new(raw);
    let count = index.validate(false)?;
    let mut patches: Vec<_> = leading_controls(raw, code)
        .into_iter()
        .map(|range| (range, String::new()))
        .collect();
    if let Some(control) = control {
        patches.push((0..0, control));
    }
    let out = apply_patches(raw, patches)?;
    index.certify(&out, count)?;
    Ok(out)
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
        // Without a whole-text `\Q` the text slants as its style does.
        self.oblique = number_text(object_number(&raw, 'Q').unwrap_or(self.object_style.oblique));
        self.char_space = object_number(&raw, 'T').map_or_else(|| "0".into(), number_text);
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
        let code = match field {
            "font" => 'f',
            "oblique" => 'Q',
            _ => 'W',
        };
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
            Kind::Control('Q') => field == "oblique",
            _ => false,
        });
        if overridden {
            ValueOrigin::Mixed
        } else {
            ValueOrigin::Style
        }
    }

    /// Where the height comes from: the style's fixed height or the text's
    /// own. `None` when the style has no fixed height to follow.
    pub(in crate::app) fn height_origin(&self) -> Option<ValueOrigin> {
        let style = self.object_style.height;
        if style <= 0. {
            return None;
        }
        let height = self.height.trim().replace(',', ".").parse::<f64>().ok();
        if height.is_none_or(|h| (h - style).abs() > 1e-9 * style.max(1.)) {
            return Some(ValueOrigin::Custom);
        }
        let raw = self.content.text();
        let sized = SourceIndex::new(&raw)
            .tokens
            .iter()
            .any(|t| matches!(t.kind, Kind::Control('H')));
        Some(if sized {
            ValueOrigin::Mixed
        } else {
            ValueOrigin::Style
        })
    }

    /// The style's value for a first-row field, as "Reset to style" names it.
    pub(in crate::app) fn style_value(&self, field: &str) -> String {
        let style = &self.object_style;
        match field {
            "font" => style.face.clone(),
            "height" => number_text(style.height),
            "width" => number_text(style.width),
            _ => format!("{}°", number_text(style.oblique)),
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
    pub(super) fn set_object_source(&mut self, result: Result<String, EditError>) -> bool {
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

    /// The oblique or tracking field with nothing selected: the whole-text
    /// `\Q`/`\T` that opens the source; 0 removes it. The field keeps what
    /// is typed.
    pub(in crate::app) fn mtext_object_number(
        &mut self,
        field: &str,
        value: String,
        number: Option<f64>,
    ) {
        let code = if field == "oblique" { 'Q' } else { 'T' };
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        ed.record_undo();
        let raw = ed.content.text();
        // The style's oblique needs no code; tracking's default is 0.
        let default = if code == 'Q' {
            ed.object_style.oblique
        } else {
            0.
        };
        if let Some(number) = number {
            // Degrees from the stored radians carry a last-bit error.
            let target = ((number - default).abs() > 1e-9).then_some(number);
            if object_number(&raw, code) != target {
                let control = target.map(|v| format!("\\{code}{};", number_text(v)));
                self.set_object_source(patch_object_control(&raw, code, control));
            }
        }
        if let Some(ed) = self.mtext_editor.as_mut() {
            match code {
                'Q' => ed.oblique = value,
                _ => ed.char_space = value,
            }
        }
        self.rebuild_mtext_preview();
    }

    /// The whole-text colour of a text with no colour of its own (a
    /// multileader's text, command-line input): a leading `\C`; ByLayer and
    /// ByBlock remove it.
    pub(in crate::app) fn mtext_object_color(&mut self, aci: u16) {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        ed.record_undo();
        let raw = ed.content.text();
        let control = (!matches!(aci, 0 | 256)).then(|| format!("\\C{aci};"));
        if self.set_object_source(patch_object_control(&raw, 'C', control)) {
            if let Some(ed) = self.mtext_editor.as_mut() {
                ed.color_aci = aci;
            }
        }
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
                // A height that followed the old style follows the new one.
                if ed.height_origin() == Some(ValueOrigin::Style) && style.height > 0. {
                    ed.height = number_text(style.height);
                    ed.height_dirty = true;
                }
                let raw = ed.content.text();
                ed.style = name;
                ed.object_style = style;
                ed.width = number_text(base_width(&raw, ed.object_style.width).unwrap_or(1.));
                ed.oblique =
                    number_text(object_number(&raw, 'Q').unwrap_or(ed.object_style.oblique));
            }
        }
        self.rebuild_mtext_preview();
    }

    /// "Reset to style" on a first-row field: drop the whole-text code that
    /// overrides the style, or give the text the style's fixed height. Codes
    /// that format only part of the text stay.
    pub(in crate::app) fn mtext_reset_to_style(&mut self, field: &str) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let raw = ed.content.text();
        let style = ed.object_style.clone();
        match field {
            "font" => self.mtext_object_font("[Style default]"),
            "width" => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.record_undo();
                }
                if self.set_object_source(patch_width(&raw, style.width, None)) {
                    if let Some(ed) = self.mtext_editor.as_mut() {
                        ed.width = number_text(style.width);
                    }
                }
                self.rebuild_mtext_preview();
            }
            "oblique" => {
                let shown = number_text(style.oblique);
                self.mtext_object_number("oblique", shown, Some(style.oblique));
            }
            // The object height, whatever is selected.
            "height" if style.height > 0. => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.record_undo();
                    ed.height = number_text(style.height);
                    ed.height_dirty = true;
                }
                self.rebuild_mtext_preview();
            }
            _ => {}
        }
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
    fn a_whole_text_font_group_is_the_object_font() {
        // A writer may wrap a whole-text font in one group around all the text.
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
        // A writer may write a font group per paragraph or per run: the text
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

    #[test]
    fn whole_text_oblique_and_tracking_patch_only_the_leading_control() {
        // Nothing selected: the rest of the source stays byte for byte, and a
        // run with its own oblique keeps it.
        let mut app = app(r"\pxqc,t4;a{\Q5;b}\Pc");
        app.mtext_apply_span_number("oblique", "15".into());
        assert_eq!(raw(&app), r"\Q15;\pxqc,t4;a{\Q5;b}\Pc");
        app.mtext_apply_span_number("tracking", "1.2".into());
        assert_eq!(raw(&app), r"\T1.2;\Q15;\pxqc,t4;a{\Q5;b}\Pc");
        // A new value replaces the leading control; 0 removes it.
        app.mtext_apply_span_number("oblique", "-10".into());
        assert_eq!(raw(&app), r"\Q-10;\T1.2;\pxqc,t4;a{\Q5;b}\Pc");
        app.mtext_apply_span_number("oblique", "0".into());
        assert_eq!(raw(&app), r"\T1.2;\pxqc,t4;a{\Q5;b}\Pc");
        // Half-typed or out of range: shown, nothing changes.
        app.mtext_apply_span_number("oblique", "9".repeat(3));
        assert_eq!(raw(&app), r"\T1.2;\pxqc,t4;a{\Q5;b}\Pc");
        assert_eq!(ed(&app).oblique, "999");
        // Each change is one undo step.
        app.mtext_undo();
        app.mtext_undo();
        assert_eq!(raw(&app), r"\Q-10;\T1.2;\pxqc,t4;a{\Q5;b}\Pc");
    }

    #[test]
    fn reopening_shows_the_whole_text_oblique_and_tracking() {
        let app = app(r"\Q12.5;\T1.1;ab{\Q3;c}");
        assert_eq!(ed(&app).oblique, "12.5");
        assert_eq!(ed(&app).char_space, "1.1");
        let app = self::app(r"a{\Q3;b}");
        assert_eq!(ed(&app).oblique, "0");
    }

    #[test]
    fn a_whole_text_colour_without_an_object_colour_is_a_leading_control() {
        let mut app = app(r"a{\C1;b}\Pc");
        app.command_mtext_input = true;
        app.mtext_apply_color(codec::types::Color::Index(3));
        assert_eq!(raw(&app), r"\C3;a{\C1;b}\Pc");
        assert_eq!(ed(&app).color_aci, 3);
        app.mtext_apply_color(codec::types::Color::ByLayer);
        assert_eq!(raw(&app), r"a{\C1;b}\Pc");
        assert_eq!(ed(&app).color_aci, 256);
    }

    #[test]
    fn clearing_formatting_keeps_the_whole_text_oblique_tracking_and_colour() {
        let mut app = app(r"\Q15;\T1.2;\C3;ab{\Q5;c}");
        app.mtext_clear_formatting();
        assert_eq!(raw(&app), r"\Q15;\T1.2;\C3;ab{c}");
    }
}
