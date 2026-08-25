// In-place MText editor: formatting toolbar + multi-line text area with a
// live viewport preview. Opened by the MTEXT command (new text) or by
// DDEDIT / double-click on an existing MText. The text area holds the raw
// MText value (plain text plus DXF inline format codes the toolbar inserts);
// the real entity is re-tessellated into the scene's preview wires on every
// change so the user sees the actual drawing result while typing.

use acadrust::entities::mtext::AttachmentPoint;
use acadrust::entities::mtext_format::{
    parse_mtext, MTextColor, MTextDocument, MTextFont, MTextParagraph, MTextParagraphAlignment,
    MTextSpan, ParagraphProperties, SpanProperties, StackingData, StackingType,
};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle, MText};
use glam::DVec3;
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
    pub pos: DVec3,
    /// The editable text buffer (raw value with inline codes).
    pub content: text_editor::Content,
    /// Structured mirror of `content`, parsed via acadrust `parse_mtext` — the
    /// target authority for the editor. Phase 1: passive mirror kept in sync on
    /// every rebuild, cross-checked against the rendered glyph boxes; later
    /// phases make it the source of truth and delete the raw-string path.
    pub doc: MTextDocument,
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
    /// Whether the toolbar colour picker popup is open.
    pub color_picker_open: bool,
    /// Per-visible-character boxes (world XY, world_offset already removed) for
    /// click-to-select in the preview, and the current selection as a visible-
    /// character range `[start, end)`.
    pub glyph_boxes: Vec<crate::entities::text_support::GlyphBox>,
    pub sel: Option<(usize, usize)>,
    /// Anchor offset for an in-progress drag selection.
    pub sel_anchor: usize,
    /// Text caret as a visible-character offset (0..=count). Used for typing
    /// directly into the preview.
    pub caret: usize,
    /// Blink phase — the caret is drawn only when true; reset to true on any
    /// edit/caret move so it's solid right after activity.
    pub caret_blink_on: bool,
    /// Canvas-space anchor where the toolbar + text area are drawn (the
    /// insertion-point click position).
    pub screen_anchor: iced::Point,
    /// The entity as it was opened, when editing an existing MText. `build_mtext`
    /// starts from this so every field the toolbar doesn't touch — columns,
    /// rotation, normal, background fill, defined height, annotative flag —
    /// survives the edit instead of resetting to `MText::default()`. `None` for
    /// a freshly created MText, which has nothing to preserve.
    pub original: Option<MText>,
}

impl MTextEditorState {
    pub fn new(pos: DVec3, initial: &str, height: f64, editing: Option<Handle>) -> Self {
        Self {
            pos,
            content: text_editor::Content::with_text(initial),
            doc: parse_mtext(initial, true),
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
            color_picker_open: false,
            glyph_boxes: Vec::new(),
            sel: None,
            sel_anchor: 0,
            caret: 0,
            caret_blink_on: true,
            attachment: AttachmentPoint::TopLeft,
            line_spacing: 1.0,
            // Default box ~20 characters wide; overwritten with the entity's
            // own width when editing an existing MText.
            rect_width: (height * 20.0).max(1.0),
            editing,
            screen_anchor: iced::Point::new(60.0, 90.0),
            original: None,
        }
    }

    pub fn height_value(&self) -> f64 {
        self.height.trim().parse::<f64>().ok().filter(|h| *h > 0.0).unwrap_or(0.25)
    }

    /// Pixels per drawing unit used by the editor preview. The dominant glyph
    /// height is always displayed at the same screen size; wrapping width must
    /// never participate in this calculation.
    pub(in crate::app) fn preview_scale(&self) -> f32 {
        let mut heights: Vec<f32> = self
            .glyph_boxes
            .iter()
            .filter(|b| b.xmax - b.xmin > 1e-6)
            .map(|b| b.ymax - b.ymin)
            .filter(|height| height.is_finite() && *height > 1e-6)
            .collect();
        let unit = if heights.is_empty() {
            self.height_value() as f32
        } else {
            heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let (mut best_height, mut best_count, mut index) = (heights[0], 0usize, 0usize);
            while index < heights.len() {
                let mut end = index;
                while end < heights.len() && heights[end] <= heights[index] * 1.05 {
                    end += 1;
                }
                if end - index > best_count {
                    best_count = end - index;
                    best_height = heights[(index + end - 1) / 2];
                }
                index = end;
            }
            best_height
        };
        (super::view::overlay::MTEXT_PREVIEW_EM_PX / unit.max(1e-6)).clamp(1e-4, 1e6)
    }

    /// Fold the toolbar's global defaults (font / colour / oblique / width /
    /// char-spacing) onto every span that does not already override them. This
    /// replaces the old hand-built `\f;\C;…` prefix so the value is produced
    /// purely by acadrust's `to_mtext_string`.
    fn apply_globals(&self, doc: &mut MTextDocument) {
        let font = self.font.trim();
        let color =
            (self.color_aci != 256 && self.color_aci != 0).then(|| MTextColor::Index(self.color_aci));
        let oblique = parse_non_default(&self.oblique, 0.0);
        let width = parse_non_default(&self.width, 1.0);
        let tracking = parse_non_default(&self.char_space, 0.0);
        for para in &mut doc.paragraphs {
            for span in &mut para.spans {
                let p = &mut span.properties;
                if !font.is_empty() && p.font.is_none() {
                    p.font = Some(MTextFont::from_name(font));
                }
                if p.color.is_none() && p.color_rgb.is_none() {
                    if let Some(ref c) = color {
                        p.color = Some(c.clone());
                    }
                }
                if p.oblique_angle.is_none() {
                    p.oblique_angle = oblique;
                }
                if p.width_factor.is_none() {
                    p.width_factor = width;
                }
                if p.tracking.is_none() {
                    p.tracking = tracking;
                }
            }
        }
    }

    /// The committed / previewed MText value: the structured body with the
    /// global toolbar defaults folded in, serialized by acadrust.
    fn folded_value(&self) -> String {
        let raw = self.content.text();
        let mut doc = parse_mtext(&raw, true);
        // parse_mtext drops a trailing empty paragraph; restore it so Enter at
        // the end keeps its new (empty) line.
        if content_has_trailing_break(&raw) {
            doc.paragraphs.push(MTextParagraph::new());
        }
        self.apply_globals(&mut doc);
        doc.to_mtext_string()
    }

    /// Build the MText entity from the current editor state for preview/commit.
    ///
    /// Starts from the original entity (when editing one) so fields the toolbar
    /// never exposes — columns above all, plus rotation, normal, defined height,
    /// background fill, annotative flag — round-trip untouched. Rebuilding from
    /// `MText::default()` here is what silently stripped a columnar MTEXT's
    /// columns the moment it was edited.
    pub fn build_mtext(&self) -> MText {
        let mut mt = self.original.clone().unwrap_or_default();
        mt.value = self.folded_value();
        mt.insertion_point = Vector3::new(self.pos.x, self.pos.y, self.pos.z);
        mt.height = self.height_value();
        mt.rectangle_width = self.rect_width;
        mt.attachment_point = self.attachment;
        mt.line_spacing_factor = self.line_spacing as f64;
        mt.style = self.style.clone();
        mt
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

/// One editable unit in the MText editor's flat visible-index space — exactly
/// one per caret slot, replacing the raw-string `visible_spans` walker for
/// editing. The editor flattens its `doc` to a `Vec<Cell>`, splices/drains it
/// (trivial), and rebuilds a structured `MTextDocument`; the flat index IS the
/// caret index, so caret/selection stay verbatim.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    /// A visible character carrying its span's formatting. Tabs count as a
    /// normal char here (one slot), matching the previous caret model.
    Char(char, SpanProperties),
    /// A paragraph break, carrying the paragraph properties (alignment,
    /// indents…) of the paragraph it opens, so they survive an edit round-trip.
    /// The `bool` is `starts_column`: `true` for a `\N` column break, `false`
    /// for a plain `\P`. Without it every edit flattens `\N` to `\P` and a
    /// columnar MTEXT loses its columns on the first keystroke.
    Break(ParagraphProperties, bool),
    /// One flattened glyph of a stacking (`\S`) span, kept atomic: continuation
    /// cells (`head=false`) extend the run begun by a `head=true` one, and edits
    /// never split a run.
    Stack {
        data: StackingData,
        props: SpanProperties,
        head: bool,
    },
}

impl Cell {
    fn is_stack_cont(&self) -> bool {
        matches!(self, Cell::Stack { head: false, .. })
    }
}

/// The flattened `num<sep>den` glyphs of a stacking span, in reading order.
fn flatten_stack(st: &StackingData) -> String {
    let sep = match st.stacking_type {
        StackingType::Limit => '^',
        StackingType::Diagonal => '#',
        StackingType::Horizontal => '/',
    };
    let mut flat = st.numerator.clone();
    if !st.denominator.is_empty() {
        flat.push(sep);
        flat.push_str(&st.denominator);
    }
    flat
}

/// Flatten a document into one cell per visible slot (same order + count as the
/// layout's glyph boxes). This is the editor's caret index space. The first
/// paragraph's properties are the implicit para-0 default (see [`doc_para0`]);
/// each `Break` carries the properties of the paragraph it opens.
fn doc_to_cells(doc: &MTextDocument) -> Vec<Cell> {
    let mut cells = Vec::new();
    for (pi, para) in doc.paragraphs.iter().enumerate() {
        if pi > 0 {
            cells.push(Cell::Break(para.properties.clone(), para.starts_column));
        }
        for span in &para.spans {
            if let Some(st) = &span.stacking {
                let mut head = true;
                for _ in flatten_stack(st).chars() {
                    cells.push(Cell::Stack {
                        data: st.clone(),
                        props: span.properties.clone(),
                        head,
                    });
                    head = false;
                }
            } else {
                for ch in span.text.chars() {
                    cells.push(Cell::Char(ch, span.properties.clone()));
                }
            }
        }
    }
    cells
}

/// The first paragraph's properties (the para-0 default not carried by any
/// `Break`), or the default for an empty document.
fn doc_para0(doc: &MTextDocument) -> ParagraphProperties {
    doc.paragraphs
        .first()
        .map(|p| p.properties.clone())
        .unwrap_or_default()
}

/// Rebuild a document from an edited cell list. Adjacent `Char` cells with equal
/// properties coalesce into one span; a `Break` starts a new paragraph carrying
/// its stored properties; a `Stack` run's head cell re-emits the stacking span.
/// `para0` supplies the first paragraph's properties.
fn cells_to_doc(para0: &ParagraphProperties, cells: &[Cell]) -> MTextDocument {
    let mut doc = MTextDocument::new();
    doc.paragraphs.clear();
    let mut para = MTextParagraph::new();
    para.properties = para0.clone();
    for cell in cells {
        match cell {
            Cell::Break(props, starts_column) => {
                doc.paragraphs
                    .push(std::mem::replace(&mut para, MTextParagraph::new()));
                para.properties = props.clone();
                para.starts_column = *starts_column;
            }
            Cell::Char(ch, props) => match para.spans.last_mut() {
                Some(s) if s.stacking.is_none() && &s.properties == props => s.text.push(*ch),
                _ => para.spans.push(MTextSpan::new(ch.to_string(), props.clone())),
            },
            Cell::Stack { data, props, head } => {
                if *head {
                    let mut s = MTextSpan::new(String::new(), props.clone());
                    s.stacking = Some(data.clone());
                    para.spans.push(s);
                }
            }
        }
    }
    doc.paragraphs.push(para);
    doc
}

/// The paragraph properties in force at flat index `caret` — the last `Break`
/// before it, or the para-0 default. Used so a newly inserted break clones the
/// current paragraph's properties.
fn para_props_at(para0: &ParagraphProperties, cells: &[Cell], caret: usize) -> ParagraphProperties {
    let mut props = para0.clone();
    for cell in cells.iter().take(caret) {
        if let Cell::Break(p, _) = cell {
            props = p.clone();
        }
    }
    props
}

/// Turn an inserted string (which may carry `\P` breaks) into cells, tagging
/// every plain char with `span_props` and every break with `para_props`.
fn str_to_cells(s: &str, span_props: &SpanProperties, para_props: &ParagraphProperties) -> Vec<Cell> {
    let mut cells = Vec::new();
    for (i, seg) in s.split("\\P").enumerate() {
        if i > 0 {
            // A typed / pasted break is a plain `\P`; the editor offers no way to
            // insert a column break, so existing `\N`s are the only columnar ones.
            cells.push(Cell::Break(para_props.clone(), false));
        }
        for ch in seg.chars() {
            cells.push(Cell::Char(ch, span_props.clone()));
        }
    }
    cells
}

/// Properties newly-typed text at `caret` inherits — the cell to its left, or
/// the default at the start of a paragraph / document.
fn insert_props(cells: &[Cell], caret: usize) -> SpanProperties {
    match caret.checked_sub(1).and_then(|i| cells.get(i)) {
        Some(Cell::Char(_, p)) | Some(Cell::Stack { props: p, .. }) => p.clone(),
        _ => SpanProperties::default(),
    }
}

/// Nudge an insertion index out of the interior of a stacking run (never split
/// a fraction): step forward past any continuation cells.
fn clamp_insert(cells: &[Cell], mut caret: usize) -> usize {
    while caret < cells.len() && cells[caret].is_stack_cont() {
        caret += 1;
    }
    caret
}

/// True when the serialized MText value ends with a hard paragraph break —
/// i.e. a trailing empty paragraph, which `parse_mtext` drops. `to_mtext_string`
/// wraps multi-paragraph output in `{…}`, so strip a trailing `}` first, then
/// require an ODD run of backslashes before a final `P` (an even run is an
/// escaped literal `\\`, not the `\P` break code).
fn content_has_trailing_break(raw: &str) -> bool {
    let s = raw.trim_end();
    let s = s.strip_suffix('}').unwrap_or(s);
    match s.strip_suffix('P') {
        Some(rest) => rest.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1,
        None => false,
    }
}

/// Character class for double-click word selection: a word run, a whitespace
/// run, or a paragraph break (each selects a maximal same-class run).
#[derive(PartialEq)]
enum Class {
    Word,
    Space,
    Break,
}

fn cell_class(c: &Cell) -> Class {
    match c {
        Cell::Break(..) => Class::Break,
        Cell::Char(ch, _) if ch.is_whitespace() => Class::Space,
        // A char or an atomic stacking glyph is part of a word.
        _ => Class::Word,
    }
}

/// The `[start, end)` visible range of the word at flat index `off` — the
/// maximal run of the same character class around it. Used for double-click.
fn word_range(cells: &[Cell], off: usize) -> (usize, usize) {
    let n = cells.len();
    if n == 0 {
        return (0, 0);
    }
    // Reference the cell at `off`, or the one before it when past the end.
    let idx = off.min(n - 1);
    let class = cell_class(&cells[idx]);
    if class == Class::Break {
        return (idx, idx + 1);
    }
    let mut a = idx;
    while a > 0 && cell_class(&cells[a - 1]) == class {
        a -= 1;
    }
    let mut b = idx + 1;
    while b < n && cell_class(&cells[b]) == class {
        b += 1;
    }
    (a, b)
}

/// Delete cells `[a, b)`, first widening the range so it never cuts a stacking
/// run in half. Returns the (possibly widened) start index — the new caret.
fn cells_delete_range(cells: &mut Vec<Cell>, mut a: usize, mut b: usize) -> usize {
    let n = cells.len();
    a = a.min(n);
    b = b.clamp(a, n);
    // Never start inside a stacking run — back up to its head cell.
    while a < cells.len() && cells[a].is_stack_cont() {
        a -= 1;
    }
    // ...and never stop inside one — extend to the run's end.
    while b < cells.len() && cells[b].is_stack_cont() {
        b += 1;
    }
    cells.drain(a..b);
    a
}


// ── App-side editor driver ──────────────────────────────────────────────────

use crate::scene::convert::tessellate;
use crate::scene::model::wire_model::WireModel;

fn vertical_caret_target(
    boxes: &[crate::entities::text_support::GlyphBox],
    caret: usize,
    visible_count: usize,
    direction: i8,
) -> usize {
    if boxes.is_empty() || direction == 0 {
        return caret.min(visible_count);
    }

    let anchor = boxes
        .iter()
        .find(|item| item.vis == caret)
        .map(|item| (item.xmin, item.ymin, item.ymax))
        .or_else(|| {
            caret.checked_sub(1).and_then(|previous| {
                boxes
                    .iter()
                    .find(|item| item.vis == previous)
                    .map(|item| (item.xmax, item.ymin, item.ymax))
            })
        })
        .or_else(|| boxes.first().map(|item| (item.xmin, item.ymin, item.ymax)));
    let Some((anchor_x, anchor_y, anchor_top)) = anchor else {
        return caret.min(visible_count);
    };

    let sign = f32::from(direction.signum());
    let line_epsilon = ((anchor_top - anchor_y).abs() * 0.15).max(1e-5);
    let nearest_line = boxes
        .iter()
        .filter_map(|item| {
            let distance = (item.ymin - anchor_y) * sign;
            (distance > line_epsilon).then_some(distance)
        })
        .fold(f32::INFINITY, f32::min);
    if !nearest_line.is_finite() {
        return caret.min(visible_count);
    }

    let line_tolerance = line_epsilon.max(nearest_line * 0.15);
    let mut best = (f32::INFINITY, caret.min(visible_count));
    for item in boxes {
        let distance = (item.ymin - anchor_y) * sign;
        if (distance - nearest_line).abs() > line_tolerance {
            continue;
        }
        for (x, offset) in [(item.xmin, item.vis), (item.xmax, item.vis + 1)] {
            let score = (x - anchor_x).abs();
            if score < best.0 {
                best = (score, offset.min(visible_count));
            }
        }
    }
    best.1
}

impl super::OpenCADStudio {
    /// Open the in-place editor for a new (`handle = None`) or existing MText.
    /// Open the rich MText editor for a new or existing MText / MultiLeader.
    /// The committed slot is chosen by the edited entity's type.
    pub(super) fn open_mtext_editor(
        &mut self,
        pos: DVec3,
        handle: Option<Handle>,
        initial: &str,
        height: f64,
    ) {
        if handle.is_some_and(|h| self.tabs[self.active_tab].scene.is_layer_locked(h)) {
            return;
        }
        let mut state = MTextEditorState::new(pos, initial, height, handle);
        if let Some(p) = self.tabs[self.active_tab].scene.selection.borrow().last_move_pos {
            state.screen_anchor = p;
        }
        // Seed attachment / line-spacing / box width from the entity being edited.
        if let Some(h) = handle {
            match self.tabs[self.active_tab].scene.document.get_entity(h) {
                Some(EntityType::MText(m)) => {
                    state.attachment = m.attachment_point;
                    state.line_spacing = m.line_spacing_factor as f32;
                    if !m.style.trim().is_empty() {
                        state.style = m.style.clone();
                    }
                    if m.rectangle_width > 0.0 {
                        state.rect_width = m.rectangle_width;
                    }
                    // Keep the whole entity so `build_mtext` preserves the fields
                    // the toolbar can't edit (columns, rotation, background…).
                    state.original = Some(m.clone());
                }
                Some(EntityType::MultiLeader(ml)) => {
                    state.line_spacing = ml.context.line_spacing_factor as f32;
                    if ml.context.text_width > 0.0 {
                        state.rect_width = ml.context.text_width;
                    }
                }
                _ => {}
            }
        } else {
            // New MText inherits the document's current text style (STYLE),
            // not the "Standard" default. See #92.
            let cur_style = self.tabs[self.active_tab]
                .scene
                .document
                .header
                .current_text_style_name
                .clone();
            if !cur_style.trim().is_empty() {
                state.style = cur_style;
            }
        }
        self.mtext_editor = Some(state);
        // Open centred at the editor's natural size: it renders through the
        // shared modal frame, which `modal_offset` positions and `modal_resize`
        // grows (both draggable, so reset on open).
        self.reset_modal_geometry();
        self.rebuild_mtext_preview();
        // The preview uses a fixed on-screen text size. Therefore the initial
        // wrap width is the drawing-unit span that exactly reaches the right
        // edge of the editor, placing both ruler and slider at their maximum.
        if handle.is_none() {
            let initial_width = self.mtext_editor.as_ref().map(|ed| {
                super::view::overlay::MTEXT_EDITOR_WRITING_WIDTH / ed.preview_scale()
            });
            if let (Some(ed), Some(width)) = (self.mtext_editor.as_mut(), initial_width) {
                ed.rect_width = f64::from(width.max(1e-6));
            }
            self.rebuild_mtext_preview();
        }
        // Place the caret at the end so typing works without a click first.
        let end = self.mtext_vis_count();
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.caret = end;
            ed.sel = Some((end, end));
            ed.sel_anchor = end;
        }
    }

    /// Re-tessellate the current editor text into the editor's OWN preview
    /// strokes (stored on the state, drawn in the dedicated preview area —
    /// never on the drawing canvas).
    pub(super) fn rebuild_mtext_preview(&mut self) {
        let i = self.active_tab;
        let Some(ed) = self.mtext_editor.as_ref() else { return };
        let mut mt = ed.build_mtext();
        // A trailing empty paragraph (a fresh line after Enter) is dropped by the
        // layout's parser, so it emits no caret box there and the caret would be
        // invisible on that line. Inject a space into the PREVIEW value only
        // (never committed) so the layout lays the line out and the caret gets a
        // box on it. Keyed off the current content — `ed.doc` is only re-synced
        // further down in this function, so it is still stale here.
        if content_has_trailing_break(&ed.content.text()) {
            match mt.value.rfind('}') {
                Some(pos) => mt.value.insert(pos, ' '),
                None => mt.value.push(' '),
            }
        }
        // Lay the preview out at the origin, not at the entity's real insertion.
        // The panel fits the strokes to its own box and never places them on the
        // drawing, so world position is irrelevant here — and a UTM-scale
        // insertion cast to f32 (glyph boxes) or split high/low (strokes, whose
        // low half this 2-D canvas drops) garbles every glyph through
        // catastrophic cancellation. Insertion is a pure translation, so zeroing
        // it keeps the layout's shape, wrapping and alignment identical.
        mt.insertion_point = Vector3::new(0.0, 0.0, 0.0);
        let entity = EntityType::MText(mt.clone());
        let anno = 1.0;
        let bg = self.tabs[i].scene.bg_color;
        let wires: Vec<WireModel> = tessellate::tessellate(
            &self.tabs[i].scene.document,
            ed.editing.unwrap_or(Handle::new(1)),
            &entity,
            false,
            [0.92, 0.92, 0.92, 1.0],
            0.0,
            [0.0; 8],
            1.0,
            anno,
            None,
            None,
            bg,
            // Editor preview draws on a 2D canvas with no SDF shader — force the
            // glyph outline strokes so the text is visible (#308).
            true,
        );
        // Per-character boxes share the preview wires' absolute coordinate frame.
        let boxes = crate::entities::mtext::glyph_boxes(&mt, &self.tabs[i].scene.document);
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.preview_wires = wires;
            ed.glyph_boxes = boxes;
            // Re-sync the structured doc from the (authoritative) content — the
            // single sync point, since every edit path ends in a rebuild.
            // `parse_mtext` drops a trailing empty paragraph, so restore it when
            // the content ends in a hard break, else Enter at the end would lose
            // the new line.
            let raw = ed.content.text();
            let mut d = parse_mtext(&raw, true);
            if content_has_trailing_break(&raw) {
                d.paragraphs.push(MTextParagraph::new());
            }
            ed.doc = d;
        }
    }

    /// Splice text around the preview selection (visible-char range) in the
    /// raw value. `case` optionally transforms the selected slice. Returns
    /// true when a preview selection was present and applied.
    /// Toggle a character format over the preview selection, on the structured
    /// span properties. The stroke-font renderer has no true bold, so Bold
    /// switches the run to the heavier "Gothic" face; Italic applies a 15°
    /// oblique; underline / overline / strike flip the stroke flags; case
    /// rewrites the glyphs. All are true ON/OFF toggles.
    pub(super) fn mtext_apply_fmt(&mut self, kind: MTextFmt) {
        // Does every span-carrying cell in the range satisfy `get`?
        fn all_have(cells: &[Cell], get: impl Fn(&SpanProperties) -> bool) -> bool {
            cells.iter().all(|c| match c {
                Cell::Char(_, p) | Cell::Stack { props: p, .. } => get(p),
                Cell::Break(..) => true,
            })
        }
        // Apply `f` to every span-carrying cell's properties in the range.
        fn each(cells: &mut [Cell], mut f: impl FnMut(&mut SpanProperties)) {
            for c in cells {
                match c {
                    Cell::Char(_, p) | Cell::Stack { props: p, .. } => f(p),
                    Cell::Break(..) => {}
                }
            }
        }
        // The effective font name to stamp on bold runs. `to_mtext_string` drops
        // a font code whose name is empty, which would lose the bold flag on the
        // round-trip; stamping the resolved style (or global) font keeps bold AND
        // renders the same font, just with the wider bold pen.
        let bold_font: String = if matches!(kind, MTextFmt::Bold) {
            let doc = &self.tabs[self.active_tab].scene.document;
            self.mtext_editor
                .as_ref()
                .map(|ed| {
                    if !ed.font.trim().is_empty() {
                        ed.font.clone()
                    } else {
                        crate::entities::text_support::resolve_text_style(&ed.style, doc)
                            .font_name
                    }
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        if let Some(ed) = self.mtext_editor.as_mut() {
            let Some((a, b)) = ed.sel.filter(|&(a, b)| a < b) else {
                return;
            };
            let para0 = doc_para0(&ed.doc);
            let mut cells = doc_to_cells(&ed.doc);
            let b = b.min(cells.len());
            if a >= b {
                return;
            }
            match kind {
                MTextFmt::Underline => {
                    let on = !all_have(&cells[a..b], |p| p.underline());
                    each(&mut cells[a..b], |p| p.set_underline(on));
                }
                MTextFmt::Overline => {
                    let on = !all_have(&cells[a..b], |p| p.overline());
                    each(&mut cells[a..b], |p| p.set_overline(on));
                }
                MTextFmt::Strike => {
                    let on = !all_have(&cells[a..b], |p| p.strikethrough());
                    each(&mut cells[a..b], |p| p.set_strikethrough(on));
                }
                MTextFmt::Italic => {
                    // Italic is the editor's canonical 15° oblique. An imported
                    // run may instead carry the font italic flag (`\f…|i1;`);
                    // recognise that as italic too, and clear it on toggle so
                    // the one canonical form is the oblique — otherwise an
                    // imported italic run could never be switched off.
                    let on = !all_have(&cells[a..b], |p| {
                        p.oblique_angle == Some(15.0)
                            || p.font.as_ref().is_some_and(|f| f.italic)
                    });
                    each(&mut cells[a..b], |p| {
                        p.oblique_angle = on.then_some(15.0);
                        if let Some(f) = p.font.as_mut() {
                            f.italic = false;
                        }
                    });
                }
                MTextFmt::Bold => {
                    // Real bold: set the font's bold flag (the SDF renderer bakes
                    // a wider pen), keeping the run's font name + italic.
                    let on = !all_have(&cells[a..b], |p| {
                        p.font.as_ref().is_some_and(|f| f.bold)
                    });
                    each(&mut cells[a..b], |p| {
                        let (name, italic) = p
                            .font
                            .as_ref()
                            .map(|f| (f.name.clone(), f.italic))
                            .unwrap_or_default();
                        // Bold needs a non-empty name to serialize; stamp the
                        // effective font when the run had none.
                        let name = if name.trim().is_empty() {
                            bold_font.clone()
                        } else {
                            name
                        };
                        p.font = if on {
                            Some(MTextFont::with_flags(name, true, italic))
                        } else if !italic && (name.is_empty() || name == bold_font) {
                            None // bold was the only reason for the font override
                        } else {
                            Some(MTextFont::with_flags(name, false, italic))
                        };
                    });
                }
                MTextFmt::Uppercase | MTextFmt::Lowercase => {
                    let up = kind == MTextFmt::Uppercase;
                    for c in &mut cells[a..b] {
                        if let Cell::Char(ch, _) = c {
                            let mapped = if up {
                                ch.to_uppercase().next()
                            } else {
                                ch.to_lowercase().next()
                            };
                            *ch = mapped.unwrap_or(*ch);
                        }
                    }
                }
            }
            ed.content =
                text_editor::Content::with_text(&cells_to_doc(&para0, &cells).to_mtext_string());
            ed.sel = Some((a, b));
            ed.caret = b;
            ed.caret_blink_on = true;
        }
        self.rebuild_mtext_preview();
    }

    /// Set the alignment of every paragraph the selection (or caret) touches.
    pub(super) fn mtext_apply_align(&mut self, align: ParaAlign) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            let para0 = doc_para0(&ed.doc);
            let cells = doc_to_cells(&ed.doc);
            let (a, b) = ed
                .sel
                .filter(|&(a, b)| a < b)
                .unwrap_or((ed.caret, ed.caret));
            let a = a.min(cells.len());
            let b = b.min(cells.len());
            let want = Some(match align {
                ParaAlign::Left => MTextParagraphAlignment::Left,
                ParaAlign::Center => MTextParagraphAlignment::Center,
                ParaAlign::Right => MTextParagraphAlignment::Right,
                ParaAlign::Justify => MTextParagraphAlignment::Justified,
            });
            // Paragraph index of a cell = number of breaks before it.
            let para_of =
                |idx: usize| cells.iter().take(idx).filter(|c| matches!(c, Cell::Break(..))).count();
            let last_cell = if b > a { b - 1 } else { a };
            let first = para_of(a);
            let last = para_of(last_cell);
            let mut doc = cells_to_doc(&para0, &cells);
            let end = last.min(doc.paragraphs.len().saturating_sub(1));
            for p in first..=end {
                doc.paragraphs[p].properties.alignment = want;
            }
            ed.content = text_editor::Content::with_text(&doc.to_mtext_string());
            ed.caret_blink_on = true;
        }
        self.rebuild_mtext_preview();
    }

    // ── Caret editing directly on the preview ───────────────────────────────

    /// Insert text (or replace the current selection) at the preview caret.
    pub(super) fn mtext_type(&mut self, s: &str) {
        // MTEXT stores a hard line break as the `\P` code. A literal newline —
        // from Enter or a multi-line paste — is not a break to the layout (only
        // the caret logic honours it), so the preview would stay on one line.
        // Normalise every literal newline to `\P` so it breaks and the saved
        // value stays standard (#308).
        let normalized;
        let s: &str = if s.contains(['\n', '\r']) {
            normalized = s
                .replace("\r\n", "\\P")
                .replace('\n', "\\P")
                .replace('\r', "\\P");
            &normalized
        } else {
            s
        };
        if let Some(ed) = self.mtext_editor.as_mut() {
            // Edit the structured document as a flat cell list, then serialize
            // it back into the raw content the preview/commit still read.
            let para0 = doc_para0(&ed.doc);
            let mut cells = doc_to_cells(&ed.doc);
            let count = cells.len();
            let mut caret = match ed.sel {
                Some((a, b)) if a < b && b <= count => cells_delete_range(&mut cells, a, b),
                _ => ed.caret.min(count),
            };
            caret = clamp_insert(&cells, caret);
            let props = insert_props(&cells, caret);
            let para_props = para_props_at(&para0, &cells, caret);
            let ins = str_to_cells(s, &props, &para_props);
            let added = ins.len();
            cells.splice(caret..caret, ins);
            caret += added;
            ed.content =
                text_editor::Content::with_text(&cells_to_doc(&para0, &cells).to_mtext_string());
            ed.caret = caret;
            ed.sel = Some((caret, caret));
            ed.sel_anchor = caret;
            ed.caret_blink_on = true;
        }
        self.rebuild_mtext_preview();
    }

    /// Delete the selection, or the visible character before the caret.
    pub(super) fn mtext_backspace(&mut self) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            let para0 = doc_para0(&ed.doc);
            let mut cells = doc_to_cells(&ed.doc);
            let count = cells.len();
            let caret = match ed.sel {
                Some((a, b)) if a < b && b <= count => cells_delete_range(&mut cells, a, b),
                _ if ed.caret > 0 && ed.caret <= count => {
                    cells_delete_range(&mut cells, ed.caret - 1, ed.caret)
                }
                _ => ed.caret.min(count),
            };
            ed.content =
                text_editor::Content::with_text(&cells_to_doc(&para0, &cells).to_mtext_string());
            ed.caret = caret;
            ed.sel = Some((caret, caret));
            ed.sel_anchor = caret;
            ed.caret_blink_on = true;
        }
        self.rebuild_mtext_preview();
    }

    /// Delete the selection, or the visible character at the caret.
    pub(super) fn mtext_delete(&mut self) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            let para0 = doc_para0(&ed.doc);
            let mut cells = doc_to_cells(&ed.doc);
            let count = cells.len();
            let caret = match ed.sel {
                Some((a, b)) if a < b && b <= count => cells_delete_range(&mut cells, a, b),
                _ if ed.caret < count => {
                    cells_delete_range(&mut cells, ed.caret, ed.caret + 1)
                }
                _ => ed.caret.min(count),
            };
            ed.content =
                text_editor::Content::with_text(&cells_to_doc(&para0, &cells).to_mtext_string());
            ed.caret = caret;
            ed.sel = Some((caret, caret));
            ed.sel_anchor = caret;
            ed.caret_blink_on = true;
        }
        self.rebuild_mtext_preview();
    }

    /// Move the caret horizontally by `delta` visible characters.
    pub(super) fn mtext_caret_move(&mut self, delta: i32, extend_selection: bool) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            let n = doc_to_cells(&ed.doc).len() as i32;
            let c = if extend_selection {
                (ed.caret as i32 + delta).clamp(0, n) as usize
            } else {
                match ed.sel {
                    Some((start, end)) if start < end && delta < 0 => start.min(n as usize),
                    Some((start, end)) if start < end && delta > 0 => end.min(n as usize),
                    _ => (ed.caret as i32 + delta).clamp(0, n) as usize,
                }
            };
            ed.caret = c;
            if extend_selection {
                ed.sel = Some((ed.sel_anchor.min(c), ed.sel_anchor.max(c)));
            } else {
                ed.sel = Some((c, c));
                ed.sel_anchor = c;
            }
            ed.caret_blink_on = true;
        }
    }

    /// Move the caret to the visually adjacent text line while preserving its
    /// horizontal position as closely as the laid-out glyph boxes allow.
    pub(super) fn mtext_caret_move_vertical(
        &mut self,
        direction: i8,
        extend_selection: bool,
    ) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            let visible_count = doc_to_cells(&ed.doc).len();
            let caret = vertical_caret_target(
                &ed.glyph_boxes,
                ed.caret,
                visible_count,
                direction,
            );
            ed.caret = caret;
            if extend_selection {
                ed.sel = Some((ed.sel_anchor.min(caret), ed.sel_anchor.max(caret)));
            } else {
                ed.sel = Some((caret, caret));
                ed.sel_anchor = caret;
            }
            ed.caret_blink_on = true;
        }
    }

    /// Visible-character count of the current text.
    pub(super) fn mtext_vis_count(&self) -> usize {
        self.mtext_editor
            .as_ref()
            .map(|ed| doc_to_cells(&ed.doc).len())
            .unwrap_or(0)
    }

    /// Select the whole editor text (Ctrl+A / triple-click).
    pub(super) fn mtext_select_all(&mut self) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            let n = doc_to_cells(&ed.doc).len();
            ed.sel_anchor = 0;
            ed.sel = Some((0, n));
            ed.caret = n;
            ed.caret_blink_on = true;
        }
    }

    /// Apply a font choice: to the current selection (per-run) when there is
    /// one, otherwise to the whole text via the global font. `font` is a family
    /// name or `"[Style default]"`.
    pub(super) fn mtext_apply_font(&mut self, font: &str) {
        let default = font == "[Style default]";
        // Effective name to stamp — never empty (so bold/italic survive
        // serialization): the picked font, or the resolved style font.
        let name: String = if default {
            let doc = &self.tabs[self.active_tab].scene.document;
            self.mtext_editor
                .as_ref()
                .map(|ed| {
                    crate::entities::text_support::resolve_text_style(&ed.style, doc).font_name
                })
                .unwrap_or_default()
        } else {
            font.to_string()
        };
        let has_sel = self
            .mtext_editor
            .as_ref()
            .and_then(|e| e.sel)
            .is_some_and(|(a, b)| a < b);
        if has_sel {
            if let Some(ed) = self.mtext_editor.as_mut() {
                let (a, b) = ed.sel.unwrap();
                let para0 = doc_para0(&ed.doc);
                let mut cells = doc_to_cells(&ed.doc);
                let b = b.min(cells.len());
                if a < b {
                    for c in &mut cells[a..b] {
                        if let Cell::Char(_, p) | Cell::Stack { props: p, .. } = c {
                            let (bold, italic) = p
                                .font
                                .as_ref()
                                .map(|f| (f.bold, f.italic))
                                .unwrap_or_default();
                            p.font = if default && !bold && !italic {
                                None // reset to inherit the style font
                            } else {
                                Some(MTextFont::with_flags(name.clone(), bold, italic))
                            };
                        }
                    }
                    ed.content = text_editor::Content::with_text(
                        &cells_to_doc(&para0, &cells).to_mtext_string(),
                    );
                    ed.sel = Some((a, b));
                    ed.caret = b;
                    ed.caret_blink_on = true;
                }
            }
        } else if let Some(ed) = self.mtext_editor.as_mut() {
            // No selection: the global font applies to the whole text.
            ed.font = if default { String::new() } else { font.to_string() };
        }
        self.rebuild_mtext_preview();
    }

    /// Apply a colour from the shared Properties-style picker: to the selection
    /// (per-run) when there is one, else the whole text via the global colour.
    /// Closes the picker popup.
    pub(super) fn mtext_apply_color(&mut self, color: acadrust::types::Color) {
        use acadrust::types::Color as C;
        let (mcolor, rgb): (Option<MTextColor>, Option<(u8, u8, u8)>) = match color {
            C::Index(i) => (Some(MTextColor::Index(i as u16)), None),
            C::Rgb { r, g, b } => (None, Some((r, g, b))),
            _ => (None, None), // ByLayer / ByBlock → inherit
        };
        let has_sel = self
            .mtext_editor
            .as_ref()
            .and_then(|e| e.sel)
            .is_some_and(|(a, b)| a < b);
        if has_sel {
            if let Some(ed) = self.mtext_editor.as_mut() {
                let (a, b) = ed.sel.unwrap();
                let para0 = doc_para0(&ed.doc);
                let mut cells = doc_to_cells(&ed.doc);
                let b = b.min(cells.len());
                if a < b {
                    for c in &mut cells[a..b] {
                        if let Cell::Char(_, p) | Cell::Stack { props: p, .. } = c {
                            p.color = mcolor.clone();
                            p.color_rgb = rgb;
                        }
                    }
                    ed.content = text_editor::Content::with_text(
                        &cells_to_doc(&para0, &cells).to_mtext_string(),
                    );
                    ed.sel = Some((a, b));
                    ed.caret = b;
                    ed.caret_blink_on = true;
                }
            }
        } else if let Some(ed) = self.mtext_editor.as_mut() {
            // No selection: the global colour applies (ACI only; a true colour
            // with no selection falls back to ByLayer).
            ed.color_aci = match color {
                C::Index(i) => i as u16,
                _ => 256,
            };
        }
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.color_picker_open = false;
        }
        self.rebuild_mtext_preview();
    }

    /// Select the word at visible offset `off` (double-click).
    pub(super) fn mtext_select_word(&mut self, off: usize) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            let cells = doc_to_cells(&ed.doc);
            let (a, b) = word_range(&cells, off);
            ed.sel_anchor = a;
            ed.sel = Some((a, b));
            ed.caret = b;
            ed.caret_blink_on = true;
        }
    }

    /// The currently selected text as plain text (for Ctrl+C), or `None` when
    /// the selection is empty.
    pub(super) fn mtext_selected_text(&self) -> Option<String> {
        let ed = self.mtext_editor.as_ref()?;
        let (a, b) = ed.sel.filter(|&(a, b)| a < b)?;
        let cells = doc_to_cells(&ed.doc);
        let slice = cells.get(a..b.min(cells.len()))?;
        Some(cells_to_doc(&ParagraphProperties::default(), slice).to_plain_text())
    }

    /// Commit the editor — create a new MText or update the edited one.
    pub(super) fn mtext_commit(&mut self) -> bool {
        let i = self.active_tab;
        let Some(ed) = self.mtext_editor.take() else { return false };
        let body_empty = ed.content.text().trim().is_empty();
        let mut mt = ed.build_mtext();
        let annotative = ed.editing.is_none()
            && crate::scene::annotative::text_style_is_annotative(
                &self.tabs[i].scene.document,
                &mt.style,
            );
        if annotative {
            mt.is_annotative = true;
        }
        if body_empty {
            // Empty content: drop a new entity; leave an edited one untouched.
            self.refresh_properties();
            return false;
        }
        if let Some(h) = ed.editing {
            if self.tabs[i].scene.is_layer_locked(h) {
                self.refresh_properties();
                return false;
            }
            self.push_undo_snapshot(i, "MTEXT");
            match self.tabs[i].scene.document.get_entity_mut(h) {
                Some(EntityType::MText(t)) => {
                    t.value = mt.value;
                    t.height = mt.height;
                    t.attachment_point = mt.attachment_point;
                    t.line_spacing_factor = mt.line_spacing_factor;
                    t.rectangle_width = mt.rectangle_width;
                }
                Some(EntityType::MultiLeader(ml)) => {
                    ml.context.text_string = mt.value;
                    ml.context.text_height = mt.height;
                    ml.context.line_spacing_factor = mt.line_spacing_factor;
                    if mt.rectangle_width > 0.0 {
                        ml.context.text_width = mt.rectangle_width;
                    }
                }
                _ => {}
            }
            // Keep the displayed annotation context in sync.
            if matches!(
                self.tabs[i].scene.document.get_entity(h),
                Some(EntityType::MultiLeader(_))
            ) {
                self.tabs[i]
                    .scene
                    .sync_displayed_annotation_context(h);
            }
            self.tabs[i]
                .scene
                .bump_entities(&[(h, crate::scene::ChangeKind::Modified)]);
            self.tabs[i].dirty = true;
        } else {
            let plane = if self.tabs[i].editing_model_space() {
                self.tabs[i].ucs_xform().working_plane()
            } else {
                crate::command::WorkingPlane::default()
            };
            let position = plane.to_local(glam::DVec3::new(
                mt.insertion_point.x,
                mt.insertion_point.y,
                mt.insertion_point.z,
            ));
            mt.insertion_point = acadrust::types::Vector3::new(
                position.x,
                position.y,
                position.z,
            );
            mt.rotation = 0.0;
            self.push_undo_snapshot(i, "MTEXT");
            let handle = self.commit_entity_handle(plane.place_entity(EntityType::MText(mt)));
            if annotative {
                let scale = self.tabs[i].scene.current_annotation_scale_handle();
                if let (Some(handle), Some(scale)) = (handle, scale) {
                    crate::scene::annotative::create_annotation_context(
                        &mut self.tabs[i].scene.document,
                        handle,
                        scale,
                    );
                }
            }
            self.tabs[i].dirty = true;
        }
        self.refresh_properties();
        true
    }

    /// Apply the buffer to the drawing but keep the editor open — the
    /// style-manager model, where **Apply** commits and the dialog stays up
    /// (closing is the ✕). A brand-new MText is created on the first apply and
    /// then updated in place on later applies, so repeated Apply never leaves
    /// duplicate entities behind.
    pub(super) fn mtext_apply(&mut self) {
        let i = self.active_tab;
        let (mut mt, editing, body_empty) = match self.mtext_editor.as_ref() {
            Some(ed) => (
                ed.build_mtext(),
                ed.editing,
                ed.content.text().trim().is_empty(),
            ),
            None => return,
        };
        let annotative = editing.is_none()
            && crate::scene::annotative::text_style_is_annotative(
                &self.tabs[i].scene.document,
                &mt.style,
            );
        if annotative {
            mt.is_annotative = true;
        }
        if body_empty {
            return;
        }
        if let Some(h) = editing {
            if self.tabs[i].scene.is_layer_locked(h) {
                return;
            }
            self.push_undo_snapshot(i, "MTEXT");
            match self.tabs[i].scene.document.get_entity_mut(h) {
                Some(EntityType::MText(t)) => {
                    t.value = mt.value;
                    t.height = mt.height;
                    t.attachment_point = mt.attachment_point;
                    t.line_spacing_factor = mt.line_spacing_factor;
                    t.rectangle_width = mt.rectangle_width;
                }
                Some(EntityType::MultiLeader(ml)) => {
                    ml.context.text_string = mt.value;
                    ml.context.text_height = mt.height;
                    ml.context.line_spacing_factor = mt.line_spacing_factor;
                    if mt.rectangle_width > 0.0 {
                        ml.context.text_width = mt.rectangle_width;
                    }
                }
                _ => {}
            }
            if matches!(
                self.tabs[i].scene.document.get_entity(h),
                Some(EntityType::MultiLeader(_))
            ) {
                self.tabs[i]
                    .scene
                    .sync_displayed_annotation_context(h);
            }
            self.tabs[i]
                .scene
                .bump_entities(&[(h, crate::scene::ChangeKind::Modified)]);
            self.tabs[i].dirty = true;
        } else {
            let plane = if self.tabs[i].editing_model_space() {
                self.tabs[i].ucs_xform().working_plane()
            } else {
                crate::command::WorkingPlane::default()
            };
            let position = plane.to_local(glam::DVec3::new(
                mt.insertion_point.x,
                mt.insertion_point.y,
                mt.insertion_point.z,
            ));
            mt.insertion_point = acadrust::types::Vector3::new(
                position.x,
                position.y,
                position.z,
            );
            mt.rotation = 0.0;
            self.push_undo_snapshot(i, "MTEXT");
            let handle = self.commit_entity_handle(plane.place_entity(EntityType::MText(mt)));
            self.tabs[i].dirty = true;
            if annotative {
                let scale = self.tabs[i].scene.current_annotation_scale_handle();
                if let (Some(handle), Some(scale)) = (handle, scale) {
                    crate::scene::annotative::create_annotation_context(
                        &mut self.tabs[i].scene.document,
                        handle,
                        scale,
                    );
                }
            }
            // Bind the editor to the fresh entity so the next Apply updates it.
            if let (Some(h), Some(ed)) = (handle, self.mtext_editor.as_mut()) {
                ed.editing = Some(h);
            }
        }
        self.refresh_properties();
    }

    /// Discard the editor without changing the drawing.
    pub(super) fn mtext_cancel(&mut self) {
        self.mtext_editor = None;
        self.reset_modal_geometry();
    }
}

#[cfg(test)]
mod cell_tests {
    use super::*;

    fn rt(s: &str) -> String {
        let doc = parse_mtext(s, true);
        cells_to_doc(&doc_para0(&doc), &doc_to_cells(&doc)).to_mtext_string()
    }
    fn paras(s: &str) -> Vec<String> {
        parse_mtext(s, true)
            .paragraphs
            .iter()
            .map(|p| p.spans.iter().map(|sp| sp.text.clone()).collect())
            .collect()
    }
    fn cells_of(s: &str) -> Vec<Cell> {
        doc_to_cells(&parse_mtext(s, true))
    }

    #[test]
    fn roundtrip_idempotent() {
        for s in ["hello", "a b c", "one\\Ptwo", "a\\Pb\\Pc", "trailing\\P", "a  b", "\\S1/2;"] {
            let once = rt(s);
            let twice = rt(&once);
            assert_eq!(once, twice, "not idempotent for {s:?}: {once:?} vs {twice:?}");
        }
    }

    #[test]
    fn cell_count_matches_plain() {
        assert_eq!(cells_of("ab\\Pcd").len(), 5); // a b Break c d
        // parse_mtext drops the trailing empty paragraph; the app restores it in
        // rebuild() (raw.ends_with("\\P")), so this pure-parse count is 2.
        assert_eq!(cells_of("ab\\P").len(), 2);
    }

    #[test]
    fn trailing_break_survives_doc_roundtrip() {
        // A trailing Break must round-trip at the DOC level (the level the ops
        // work at) even though to_mtext_string+parse would drop it.
        let cells = vec![
            Cell::Char('a', SpanProperties::default()),
            Cell::Char('b', SpanProperties::default()),
            Cell::Break(ParagraphProperties::default(), false),
        ];
        let back = doc_to_cells(&cells_to_doc(&ParagraphProperties::default(), &cells));
        assert_eq!(back, cells);
        // Typing after the trailing break lands on the new (empty) paragraph.
        let mut c2 = cells.clone();
        c2.splice(3..3, str_to_cells("x", &SpanProperties::default(), &ParagraphProperties::default()));
        assert_eq!(
            paras(&cells_to_doc(&ParagraphProperties::default(), &c2).to_mtext_string()),
            vec!["ab", "x"]
        );
    }

    #[test]
    fn insert_break_splits_paragraph() {
        let mut cells = cells_of("abcd");
        cells.splice(2..2, str_to_cells("\\P", &SpanProperties::default(), &ParagraphProperties::default()));
        assert_eq!(paras(&cells_to_doc(&ParagraphProperties::default(), &cells).to_mtext_string()), vec!["ab", "cd"]);
    }

    #[test]
    fn backspace_break_merges_paragraphs() {
        let mut cells = cells_of("ab\\Pcd"); // [a,b,Break,c,d]
        let caret = cells_delete_range(&mut cells, 2, 3); // delete the Break slot
        assert_eq!(caret, 2);
        assert_eq!(paras(&cells_to_doc(&ParagraphProperties::default(), &cells).to_mtext_string()), vec!["abcd"]);
    }

    #[test]
    fn delete_range_across_paragraphs() {
        let mut cells = cells_of("abc\\Pdef"); // [a,b,c,Break,d,e,f]
        cells_delete_range(&mut cells, 1, 5); // b c Break d
        assert_eq!(paras(&cells_to_doc(&ParagraphProperties::default(), &cells).to_mtext_string()), vec!["aef"]);
    }

    #[test]
    fn stacking_is_atomic() {
        let cells = cells_of("\\S1/2;");
        assert_eq!(cells.len(), 3);
        assert!(matches!(cells[0], Cell::Stack { head: true, .. }));
        assert!(cells[1].is_stack_cont() && cells[2].is_stack_cont());
        let mut c2 = cells.clone();
        let start = cells_delete_range(&mut c2, 1, 2); // interior delete widens to whole run
        assert_eq!(start, 0);
        assert!(c2.is_empty());
    }

    fn centred_para(text: &str) -> MTextParagraph {
        let mut p = MTextParagraph::new();
        p.properties.alignment = Some(MTextParagraphAlignment::Center);
        p.spans.push(MTextSpan::plain(text));
        p
    }

    #[test]
    fn paragraph_props_survive_edit() {
        // Two paragraphs, the 2nd centre-aligned; round-trip through the cell
        // model (as every edit does) must keep the alignment.
        let mut doc = MTextDocument::new();
        doc.paragraphs.clear();
        let mut p0 = MTextParagraph::new();
        p0.spans.push(MTextSpan::plain("ab"));
        doc.paragraphs.push(p0);
        doc.paragraphs.push(centred_para("cd"));
        let cells = doc_to_cells(&doc);
        let back = cells_to_doc(&doc_para0(&doc), &cells);
        assert_eq!(
            back.paragraphs[1].properties.alignment,
            Some(MTextParagraphAlignment::Center),
            "alignment must survive the cell round-trip"
        );

        // A newly inserted break clones the current paragraph's alignment.
        let mut doc2 = MTextDocument::new();
        doc2.paragraphs.clear();
        doc2.paragraphs.push(centred_para("abc"));
        let p0 = doc_para0(&doc2);
        let mut cells2 = doc_to_cells(&doc2);
        let pp = para_props_at(&p0, &cells2, 2);
        cells2.splice(2..2, str_to_cells("\\P", &SpanProperties::default(), &pp));
        let out = cells_to_doc(&p0, &cells2);
        assert_eq!(out.paragraphs.len(), 2);
        assert_eq!(
            out.paragraphs[0].properties.alignment,
            Some(MTextParagraphAlignment::Center)
        );
        assert_eq!(
            out.paragraphs[1].properties.alignment,
            Some(MTextParagraphAlignment::Center),
            "split paragraph inherits alignment"
        );
    }

    #[test]
    fn word_range_selection() {
        let cells = cells_of("hello world"); // h e l l o _ w o r l d
        assert_eq!(word_range(&cells, 2), (0, 5)); // inside "hello"
        assert_eq!(word_range(&cells, 8), (6, 11)); // inside "world"
        assert_eq!(word_range(&cells, 5), (5, 6)); // the space itself
        assert_eq!(word_range(&cells, 11), (6, 11)); // past end → last word
        // Double-click does not cross a paragraph break.
        let c2 = cells_of("ab\\Pcd"); // a b Break c d
        assert_eq!(word_range(&c2, 0), (0, 2)); // "ab"
        assert_eq!(word_range(&c2, 2), (2, 3)); // the break alone
        assert_eq!(word_range(&c2, 3), (3, 5)); // "cd"
    }

    // Per-selection colour/font must survive the doc → to_mtext_string → parse
    // round-trip so the renderer actually sees them.
    #[test]
    fn per_run_color_and_font_persist() {
        use acadrust::entities::mtext_format::{MTextColor, MTextFont};
        let mut p = SpanProperties::default();
        p.color = Some(MTextColor::Index(1)); // red
        p.font = Some(MTextFont::with_flags("Arial".to_string(), true, false)); // bold Arial
        let cells = vec![Cell::Char('X', p)];
        let s = cells_to_doc(&ParagraphProperties::default(), &cells).to_mtext_string();
        let back = parse_mtext(&s, true);
        let sp = &back.paragraphs[0].spans[0];
        assert_eq!(sp.properties.color, Some(MTextColor::Index(1)));
        let f = sp.properties.font.as_ref().unwrap();
        assert_eq!(f.name, "Arial");
        assert!(f.bold);
    }

    // The global toolbar defaults fold onto the text as real span properties
    // (replacing the old hand-built `\f;\C;…` prefix), and survive serialization.
    #[test]
    fn global_defaults_fold_into_spans() {
        let mut ed = MTextEditorState::new(DVec3::ZERO, "hello world", 2.5, None);
        ed.font = "Arial".to_string();
        ed.color_aci = 1; // red
        ed.oblique = "15".to_string();
        ed.width = "2".to_string();
        let doc = parse_mtext(&ed.folded_value(), true);
        assert_eq!(doc.to_plain_text(), "hello world");
        let p = &doc.paragraphs[0].spans[0].properties;
        assert_eq!(p.font.as_ref().map(|f| f.name.as_str()), Some("Arial"));
        assert_eq!(p.color, Some(MTextColor::Index(1)));
        assert_eq!(p.oblique_angle, Some(15.0));
        assert_eq!(p.width_factor, Some(2.0));
    }

    #[test]
    fn trailing_break_detection() {
        assert!(content_has_trailing_break("{ab\\P}")); // braced (to_mtext_string form)
        assert!(content_has_trailing_break("ab\\P")); // unbraced
        assert!(!content_has_trailing_break("{a\\Pb}")); // break in the middle
        assert!(!content_has_trailing_break("ab"));
        assert!(!content_has_trailing_break("{x\\\\P}")); // escaped literal backslash + P
    }

    #[test]
    fn enter_at_end_survives_rebuild() {
        // Type "ab", Enter at end -> cells [a, b, Break].
        let cells = vec![
            Cell::Char('a', SpanProperties::default()),
            Cell::Char('b', SpanProperties::default()),
            Cell::Break(ParagraphProperties::default(), false),
        ];
        let raw = cells_to_doc(&ParagraphProperties::default(), &cells).to_mtext_string();
        // rebuild() re-parses + restores the trailing empty paragraph.
        let mut d = parse_mtext(&raw, true);
        if content_has_trailing_break(&raw) {
            d.paragraphs.push(MTextParagraph::new());
        }
        assert_eq!(doc_to_cells(&d).len(), 3, "trailing break must survive rebuild");
    }
}
