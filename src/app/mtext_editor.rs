// In-place MText editor: formatting toolbar + multi-line text area with a
// live viewport preview. Opened by the MTEXT command (new text) or by
// DDEDIT / double-click on an existing MText. The text area holds the raw
// MText value (plain text plus DXF inline format codes the toolbar inserts);
// the real entity is re-tessellated into the scene's preview wires on every
// change so the user sees the actual drawing result while typing.

use acadrust::entities::mtext::AttachmentPoint;
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle, MText};
use glam::Vec3;
use iced::widget::text_editor;

/// Character-level format toggles applied to the current selection by the
/// toolbar. Each maps to a DXF MTEXT inline code understood by the renderer
/// in `entities/text_support.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MTextFmt {
    Bold,
    Italic,
    Underline,
    Overline,
    Strike,
    Uppercase,
    Lowercase,
}

/// Paragraph alignment written as `\pxq<l|c|r|j>;`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParaAlign {
    Left,
    Center,
    Right,
    Justify,
}

impl ParaAlign {
    pub fn code(self) -> &'static str {
        match self {
            ParaAlign::Left => "l",
            ParaAlign::Center => "c",
            ParaAlign::Right => "r",
            ParaAlign::Justify => "j",
        }
    }
}

/// `pick_list`-friendly wrapper for the 9 attachment points.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct JustifyChoice(pub AttachmentPoint);

impl JustifyChoice {
    pub const ALL: [JustifyChoice; 9] = [
        JustifyChoice(AttachmentPoint::TopLeft),
        JustifyChoice(AttachmentPoint::TopCenter),
        JustifyChoice(AttachmentPoint::TopRight),
        JustifyChoice(AttachmentPoint::MiddleLeft),
        JustifyChoice(AttachmentPoint::MiddleCenter),
        JustifyChoice(AttachmentPoint::MiddleRight),
        JustifyChoice(AttachmentPoint::BottomLeft),
        JustifyChoice(AttachmentPoint::BottomCenter),
        JustifyChoice(AttachmentPoint::BottomRight),
    ];
}

impl std::fmt::Display for JustifyChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self.0 {
            AttachmentPoint::TopLeft => "Top Left",
            AttachmentPoint::TopCenter => "Top Center",
            AttachmentPoint::TopRight => "Top Right",
            AttachmentPoint::MiddleLeft => "Middle Left",
            AttachmentPoint::MiddleCenter => "Middle Center",
            AttachmentPoint::MiddleRight => "Middle Right",
            AttachmentPoint::BottomLeft => "Bottom Left",
            AttachmentPoint::BottomCenter => "Bottom Center",
            AttachmentPoint::BottomRight => "Bottom Right",
        };
        f.write_str(s)
    }
}

/// Live state of the open MText editor. Absent (`None`) when no editor is up.
pub struct MTextEditorState {
    /// World insertion point (WCS, same convention the committed entity uses).
    pub pos: Vec3,
    /// The editable text buffer (raw value with inline codes).
    pub content: text_editor::Content,
    /// Text height, edited as a string so partial input is allowed.
    pub height: String,
    /// Text style name (entity field).
    pub style: String,
    /// Global font family applied via a leading `\f<font>;` run ("" = style default).
    pub font: String,
    /// Global colour ACI (256 = ByLayer) applied via a leading `\C<aci>;`.
    pub color_aci: u16,
    /// Global oblique angle, width factor, char spacing — leading `\Q`/`\W`/`\T`.
    pub oblique: String,
    pub width: String,
    pub char_space: String,
    /// Tessellated strokes of the current text, drawn in the editor's own
    /// preview area (NOT on the drawing canvas).
    pub preview_wires: Vec<WireModel>,
    /// Paragraph attachment / justification (entity field).
    pub attachment: AttachmentPoint,
    /// Line spacing factor (entity field).
    pub line_spacing: f32,
    /// Fixed MText box width (drawing units). The text wraps within this —
    /// it is NOT derived from the typed content, so adding characters wraps
    /// to the next line instead of stretching the box into one long line.
    pub rect_width: f64,
    /// `Some` when editing an existing entity; `None` for a fresh MText.
    pub editing: Option<Handle>,
    /// When true the panel shows the rendered preview; when false the raw
    /// code/text input. Toggled so the two never stack.
    pub show_preview: bool,
    /// Per-visible-character boxes (world XY, world_offset already removed) for
    /// click-to-select in the preview, and the current selection as a visible-
    /// character range `[start, end)`.
    pub glyph_boxes: Vec<crate::entities::text_support::GlyphBox>,
    pub sel: Option<(usize, usize)>,
    /// Anchor offset for an in-progress drag selection.
    pub sel_anchor: usize,
    /// Canvas-space anchor where the toolbar + text area are drawn (the
    /// insertion-point click position).
    pub screen_anchor: iced::Point,
}

impl MTextEditorState {
    pub fn new(pos: Vec3, initial: &str, height: f64, editing: Option<Handle>) -> Self {
        Self {
            pos,
            content: text_editor::Content::with_text(initial),
            height: format!("{:.4}", height)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string(),
            style: "Standard".to_string(),
            font: String::new(),
            color_aci: 256,
            oblique: "0".to_string(),
            width: "1".to_string(),
            char_space: "0".to_string(),
            preview_wires: Vec::new(),
            show_preview: true,
            glyph_boxes: Vec::new(),
            sel: None,
            sel_anchor: 0,
            attachment: AttachmentPoint::TopLeft,
            line_spacing: 1.0,
            // Default box ~20 characters wide; overwritten with the entity's
            // own width when editing an existing MText.
            rect_width: (height * 20.0).max(1.0),
            editing,
            screen_anchor: iced::Point::new(60.0, 90.0),
        }
    }

    pub fn height_value(&self) -> f64 {
        self.height.trim().parse::<f64>().ok().filter(|h| *h > 0.0).unwrap_or(0.25)
    }

    /// Compose the raw editor text with the global leading inline codes
    /// (font / colour / oblique / width / char-spacing) the toolbar's
    /// dropdowns and value fields set. Per-selection toggles already live
    /// inside the text.
    pub fn composed_value(&self) -> String {
        let body = self.content.text();
        let body = body.strip_suffix('\n').unwrap_or(&body);
        let mut prefix = String::new();
        if !self.font.trim().is_empty() {
            prefix.push_str(&format!("\\f{};", self.font.trim()));
        }
        if self.color_aci != 256 {
            prefix.push_str(&format!("\\C{};", self.color_aci));
        }
        if let Some(v) = parse_non_default(&self.oblique, 0.0) {
            prefix.push_str(&format!("\\Q{};", v));
        }
        if let Some(v) = parse_non_default(&self.width, 1.0) {
            prefix.push_str(&format!("\\W{};", v));
        }
        if let Some(v) = parse_non_default(&self.char_space, 0.0) {
            prefix.push_str(&format!("\\T{};", v));
        }
        format!("{prefix}{body}")
    }

    /// Build the MText entity from the current editor state for preview/commit.
    pub fn build_mtext(&self) -> MText {
        let h = self.height_value();
        MText {
            value: self.composed_value(),
            insertion_point: Vector3::new(self.pos.x as f64, self.pos.y as f64, self.pos.z as f64),
            height: h,
            rectangle_width: self.rect_width,
            attachment_point: self.attachment,
            line_spacing_factor: self.line_spacing as f64,
            style: self.style.clone(),
            ..Default::default()
        }
    }

}

/// Parse a numeric field, returning `Some(v)` only when it differs from the
/// control's default (so unchanged fields emit no inline code).
fn parse_non_default(s: &str, default: f64) -> Option<f64> {
    let v = s.trim().parse::<f64>().ok()?;
    if (v - default).abs() < 1e-9 {
        None
    } else {
        Some(v)
    }
}

/// Map each visible character of a raw MText value to its byte span
/// `(start, end)` in that raw string, in the same reading order the layout
/// counts glyph boxes (paragraphs split on `\P`/`\n`/`\N`, leading/trailing
/// spaces trimmed per paragraph, inline codes skipped). Lets a preview
/// selection (visible-char range) be spliced back into the raw value.
pub fn visible_spans(raw: &str) -> Vec<(usize, usize)> {
    let is_sp = |c: char| c == ' ' || c == '\u{00A0}';
    let mut result: Vec<(usize, usize)> = Vec::new();
    let mut para: Vec<(usize, usize, char)> = Vec::new();
    let flush = |para: &mut Vec<(usize, usize, char)>, result: &mut Vec<(usize, usize)>| {
        let s = para.iter().position(|t| !is_sp(t.2)).unwrap_or(para.len());
        let e = para.iter().rposition(|t| !is_sp(t.2)).map(|i| i + 1).unwrap_or(s);
        for t in &para[s..e] {
            result.push((t.0, t.1));
        }
        para.clear();
    };
    let mut it = raw.char_indices().peekable();
    while let Some((i, ch)) = it.next() {
        match ch {
            '\\' => match it.peek().map(|&(_, c)| c) {
                Some('P') | Some('n') | Some('N') => {
                    it.next();
                    flush(&mut para, &mut result);
                }
                Some('~') => {
                    let (j, c) = it.next().unwrap();
                    para.push((i, j + c.len_utf8(), '\u{00A0}'));
                }
                Some('\\') | Some('{') | Some('}') => {
                    let (j, c) = it.next().unwrap();
                    para.push((i, j + c.len_utf8(), c));
                }
                Some(c) if "LlOoKk".contains(c) => {
                    it.next(); // value-less toggle, no visible glyph
                }
                Some(_) => {
                    // Value code (\f… \C… \H… \pxq… \U… etc) — skip to ';'.
                    it.next();
                    while let Some(&(_, c)) = it.peek() {
                        it.next();
                        if c == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            '{' | '}' => { /* group markers — not visible */ }
            '%' if it.peek().map(|&(_, c)| c) == Some('%') => {
                it.next(); // second '%'
                match it.peek().copied() {
                    Some((k, '%')) => {
                        it.next();
                        para.push((i, k + 1, '%'));
                    }
                    Some((_, d)) if d.is_ascii_digit() => {
                        let mut last = i;
                        let mut n = 0;
                        while n < 3 {
                            match it.peek().copied() {
                                Some((m, c)) if c.is_ascii_digit() => {
                                    last = m;
                                    it.next();
                                    n += 1;
                                }
                                _ => break,
                            }
                        }
                        para.push((i, last + 1, '\u{25A1}'));
                    }
                    Some((m, c)) => {
                        it.next();
                        let g = match c {
                            'd' | 'D' => '°',
                            'c' | 'C' => 'Ø',
                            'p' | 'P' => '±',
                            other => other,
                        };
                        para.push((i, m + c.len_utf8(), g));
                    }
                    None => para.push((i, i + 1, '%')),
                }
            }
            _ => para.push((i, i + ch.len_utf8(), ch)),
        }
    }
    flush(&mut para, &mut result);
    result
}

// ── App-side editor driver ──────────────────────────────────────────────────

use crate::scene::tessellate;
use crate::scene::wire_model::WireModel;
use iced::widget::text_editor::{Action, Edit};
use std::sync::Arc;

impl super::OpenCADStudio {
    /// Open the in-place editor for a new (`handle = None`) or existing MText.
    pub(super) fn open_mtext_editor(
        &mut self,
        pos: Vec3,
        handle: Option<Handle>,
        initial: &str,
        height: f64,
    ) {
        let mut state = MTextEditorState::new(pos, initial, height, handle);
        if let Some(p) = self.tabs[self.active_tab].scene.selection.borrow().last_move_pos {
            state.screen_anchor = p;
        }
        // Seed attachment / line-spacing from the entity being edited.
        if let Some(h) = handle {
            if let Some(EntityType::MText(m)) = self.tabs[self.active_tab].scene.document.get_entity(h) {
                state.attachment = m.attachment_point;
                state.line_spacing = m.line_spacing_factor as f32;
                if !m.style.trim().is_empty() {
                    state.style = m.style.clone();
                }
                if m.rectangle_width > 0.0 {
                    state.rect_width = m.rectangle_width;
                }
            }
        }
        self.mtext_editor = Some(state);
        self.rebuild_mtext_preview();
    }

    /// Re-tessellate the current editor text into the editor's OWN preview
    /// strokes (stored on the state, drawn in the dedicated preview area —
    /// never on the drawing canvas).
    pub(super) fn rebuild_mtext_preview(&mut self) {
        let i = self.active_tab;
        let Some(ed) = self.mtext_editor.as_ref() else { return };
        let mt = ed.build_mtext();
        let entity = EntityType::MText(mt.clone());
        let woff = self.tabs[i].scene.world_offset;
        let anno = self.tabs[i].scene.annotation_scale;
        let wires: Vec<WireModel> = tessellate::tessellate(
            &self.tabs[i].scene.document,
            ed.editing.unwrap_or(Handle::new(1)),
            &entity,
            false,
            [0.92, 0.92, 0.92, 1.0],
            0.0,
            [0.0; 8],
            1.0,
            woff,
            anno,
        );
        // Per-character boxes in the same offset-subtracted frame as the
        // preview wires (tessellate removes world_offset; mirror that).
        let mut boxes = crate::entities::mtext::glyph_boxes(&mt, &self.tabs[i].scene.document);
        let (ox, oy) = (woff[0] as f32, woff[1] as f32);
        for b in &mut boxes {
            b.xmin -= ox;
            b.xmax -= ox;
            b.ymin -= oy;
            b.ymax -= oy;
        }
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.preview_wires = wires;
            ed.glyph_boxes = boxes;
        }
    }

    /// Splice text around the preview selection (visible-char range) in the
    /// raw value. `case` optionally transforms the selected slice. Returns
    /// true when a preview selection was present and applied.
    fn mtext_splice_sel(&mut self, prefix: &str, suffix: &str, case: Option<bool>) -> bool {
        let Some(ed) = self.mtext_editor.as_mut() else { return false };
        let Some((a, b)) = ed.sel else { return false };
        if a >= b {
            return false;
        }
        let raw = ed.content.text();
        let raw = raw.strip_suffix('\n').unwrap_or(&raw).to_string();
        let spans = visible_spans(&raw);
        if a >= spans.len() || b > spans.len() {
            return false;
        }
        let start = spans[a].0;
        let end = spans[b - 1].1;
        let mut s = raw;
        if let Some(upper) = case {
            let slice = &s[start..end];
            let repl = if upper { slice.to_uppercase() } else { slice.to_lowercase() };
            s.replace_range(start..end, &repl);
            // Length may change; recompute end for the suffix insert.
            let new_end = start + repl.len();
            s.insert_str(new_end, suffix);
            s.insert_str(start, prefix);
        } else {
            s.insert_str(end, suffix);
            s.insert_str(start, prefix);
        }
        ed.content = iced::widget::text_editor::Content::with_text(&s);
        ed.sel = None;
        true
    }

    /// Apply a character-format toggle to the preview selection (preferred) or
    /// the Edit-box selection. The stroke-font renderer has no true bold /
    /// italic, so Bold switches the run to the heavier Gothic stroke font and
    /// Italic applies an oblique slant — both produce a visible effect.
    pub(super) fn mtext_apply_fmt(&mut self, kind: MTextFmt) {
        // Font to restore to after a Bold run (the current global font).
        let restore = self
            .mtext_editor
            .as_ref()
            .map(|e| {
                if e.font.trim().is_empty() {
                    "Standard".to_string()
                } else {
                    e.font.clone()
                }
            })
            .unwrap_or_else(|| "Standard".to_string());
        let (pre, suf, case): (String, String, Option<bool>) = match kind {
            MTextFmt::Bold => ("\\fGothic;".into(), format!("\\f{restore};"), None),
            MTextFmt::Italic => ("\\Q15;".into(), "\\Q0;".into(), None),
            MTextFmt::Underline => ("\\L".into(), "\\l".into(), None),
            MTextFmt::Overline => ("\\O".into(), "\\o".into(), None),
            MTextFmt::Strike => ("\\K".into(), "\\k".into(), None),
            MTextFmt::Uppercase => (String::new(), String::new(), Some(true)),
            MTextFmt::Lowercase => (String::new(), String::new(), Some(false)),
        };
        if !self.mtext_splice_sel(&pre, &suf, case) {
            if let Some(ed) = self.mtext_editor.as_mut() {
                let sel = ed.content.selection().unwrap_or_default();
                let text = match case {
                    Some(true) => sel.to_uppercase(),
                    Some(false) => sel.to_lowercase(),
                    None => format!("{pre}{sel}{suf}"),
                };
                ed.content.perform(Action::Edit(Edit::Paste(Arc::new(text))));
            }
        }
        self.rebuild_mtext_preview();
    }

    /// Prefix the selection (or cursor) with a paragraph-alignment code.
    pub(super) fn mtext_apply_align(&mut self, align: ParaAlign) {
        let code = format!("\\pxq{};", align.code());
        if !self.mtext_splice_sel(&code, "", None) {
            if let Some(ed) = self.mtext_editor.as_mut() {
                let sel = ed.content.selection().unwrap_or_default();
                let text = format!("{code}{sel}");
                ed.content.perform(Action::Edit(Edit::Paste(Arc::new(text))));
            }
        }
        self.rebuild_mtext_preview();
    }

    /// Commit the editor — create a new MText or update the edited one.
    pub(super) fn mtext_commit(&mut self) {
        let i = self.active_tab;
        let Some(ed) = self.mtext_editor.take() else { return };
        let body_empty = ed.content.text().trim().is_empty();
        let mt = ed.build_mtext();
        if body_empty {
            // Empty content: drop a new entity; leave an edited one untouched.
            self.refresh_properties();
            return;
        }
        if let Some(h) = ed.editing {
            self.push_undo_snapshot(i, "MTEXT");
            if let Some(EntityType::MText(t)) =
                self.tabs[i].scene.document.get_entity_mut(h)
            {
                t.value = mt.value;
                t.height = mt.height;
                t.attachment_point = mt.attachment_point;
                t.line_spacing_factor = mt.line_spacing_factor;
                t.rectangle_width = mt.rectangle_width;
            }
            self.tabs[i].scene.bump_geometry();
            self.tabs[i].dirty = true;
        } else {
            self.push_undo_snapshot(i, "MTEXT");
            self.commit_entity(EntityType::MText(mt));
            self.tabs[i].dirty = true;
        }
        self.refresh_properties();
    }

    /// Discard the editor without changing the drawing.
    pub(super) fn mtext_cancel(&mut self) {
        self.mtext_editor = None;
    }
}
