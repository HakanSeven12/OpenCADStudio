// In-place MText editor: formatting toolbar + multi-line text area with a
// live viewport preview. Opened by the MTEXT command (new text) or by
// DDEDIT / double-click on an existing MText. The text area holds the raw
// MText value (plain text plus DXF inline format codes the toolbar inserts);
// the real entity is re-tessellated into the scene's preview wires on every
// change so the user sees the actual drawing result while typing.

use codec::entities::mtext::AttachmentPoint;
use codec::entities::mtext_format::{
    parse_mtext, MTextColor, MTextDocument, MTextFont, MTextParagraph, MTextSpan,
    ParagraphProperties, SpanProperties, StackingData, StackingType,
};
use codec::types::Vector3;
use codec::{EntityType, Handle, MText};
use glam::DVec3;
use iced::widget::text_editor;

// Lossless index of the raw MTEXT source and the source patches the editor
// edits through. Typing and deleting use them; the parts the other editing
// commands will use are allowed to be unused until they switch over.
#[allow(dead_code)]
mod raw_adapter;
mod raw_clear;
mod raw_commit;
mod raw_edit;
mod raw_geometry;
#[allow(dead_code)]
mod raw_operations;
mod raw_span;
mod raw_stack;
mod raw_style;
pub(in crate::app) use raw_stack::{FractionPart, Script};
pub(in crate::app) use raw_edit::FormatState;
pub(in crate::app) use raw_style::ValueOrigin;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParaNumber {
    FirstIndent,
    LeftIndent,
    RightIndent,
    SpaceBefore,
    SpaceAfter,
}

/// The second-row toolbar menus. At most one is open at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorMenu {
    More,
    Paragraph,
    Columns,
    Justify,
    Symbols,
    Find,
    /// The Fraction panel, opened from More formatting.
    Fraction,
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
    /// Structured mirror of `content`, parsed via opencadcodec `parse_mtext` — the
    /// target authority for the editor. Phase 1: passive mirror kept in sync on
    /// every rebuild, cross-checked against the rendered glyph boxes; later
    /// phases make it the source of truth and delete the raw-string path.
    pub doc: MTextDocument,
    /// Text height, edited as a string so partial input is allowed.
    pub height: String,
    /// Text style name (entity field).
    pub style: String,
    /// The object font: the `\f`/`\F` that opens the source ("" = the style's).
    pub font: String,
    /// Global colour ACI (256 = ByLayer) applied via a leading `\C<aci>;`.
    pub color_aci: u16,
    /// Global oblique angle and char spacing — leading `\Q`/`\T`.
    pub oblique: String,
    pub char_space: String,
    /// The width the text draws with: the style's width times the `\W` that
    /// opens the source. Kept as typed, so partial input is allowed.
    pub width: String,
    /// What the current text style supplies where the source sets nothing.
    pub object_style: raw_style::ObjectStyle,
    /// The object colour set in the editor; `None` keeps the entity's own.
    pub object_color: Option<codec::types::Color>,
    /// Tessellated strokes of the current text, drawn in the editor's own
    /// preview area (NOT on the drawing canvas).
    pub preview_wires: Vec<WireModel>,
    /// Paragraph attachment / justification (entity field).
    pub attachment: AttachmentPoint,
    /// Line spacing factor (entity field).
    pub line_spacing: f32,
    /// Object rotation in degrees, kept as typed so partial input is allowed.
    pub rotation: String,
    /// The rotation field was edited; until then the opened angle is kept
    /// exactly, never re-derived from the field's rounded degrees.
    pub(super) rotation_dirty: bool,
    /// A new text's working plane is the drawing's XY plane.
    pub(super) rotation_plane_supported: bool,
    /// Fixed MText box width (drawing units). The text wraps within this —
    /// it is NOT derived from the typed content, so adding characters wraps
    /// to the next line instead of stretching the box into one long line.
    pub rect_width: f64,
    /// The width was set by the MTEXT command's box (or its Width option):
    /// the editor keeps it instead of fitting its own size.
    pub keep_width: bool,
    /// The box width as typed in the status bar while it is not a valid
    /// width yet; `None` shows `rect_width` itself.
    pub rect_width_draft: Option<String>,
    /// A ruler drag is under way: the whole drag is one undo step.
    pub(super) box_dragging: bool,
    pub rect_height: String,
    pub annotative: bool,
    pub column_type: i16,
    pub column_count: String,
    pub column_auto_height: bool,
    pub column_flow_reversed: bool,
    pub column_width: String,
    pub column_gutter: String,
    pub paragraph_first_indent: String,
    pub paragraph_left_indent: String,
    pub paragraph_right_indent: String,
    pub paragraph_space_before: String,
    pub paragraph_space_after: String,
    pub find_text: String,
    pub replace_text: String,
    undo: Vec<raw_commit::EditorSnapshot>,
    redo: Vec<raw_commit::EditorSnapshot>,
    /// `Some` when editing an existing entity; `None` for a fresh MText.
    pub editing: Option<Handle>,
    /// When true the panel shows the rendered preview; when false the raw
    /// code/text input. Toggled so the two never stack.
    pub show_preview: bool,
    /// Whether the toolbar colour picker popup is open.
    pub color_picker_open: bool,
    /// The open toolbar menu.
    pub menu: Option<EditorMenu>,
    /// The Fraction panel's parts while it is open.
    pub fraction: Option<raw_stack::FractionDraft>,
    /// B / I / U / overline / S switched with nothing selected, for the
    /// next typed text. Any caret move or new selection drops it.
    pub(super) typing_format: raw_edit::TypingFormat,
    /// Per-visible-character boxes (world XY, world_offset already removed) for
    /// click-to-select in the preview, and the current selection as a visible-
    /// character range `[start, end)`.
    pub glyph_boxes: Vec<crate::entities::text_support::GlyphBox>,
    pub sel: Option<(usize, usize)>,
    /// A height / oblique / width / tracking field typed while text is
    /// selected: it styles the selection, not the whole text, so it is shown
    /// from here for as long as that selection stands (#2194).
    pub span_input: Option<((usize, usize), &'static str, String)>,
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
            object_style: Default::default(),
            object_color: None,
            preview_wires: Vec::new(),
            show_preview: true,
            color_picker_open: false,
            menu: None,
            fraction: None,
            typing_format: Default::default(),
            glyph_boxes: Vec::new(),
            sel: None,
            span_input: None,
            sel_anchor: 0,
            caret: 0,
            caret_blink_on: true,
            attachment: AttachmentPoint::TopLeft,
            line_spacing: 1.0,
            rotation: "0".to_string(),
            rotation_dirty: false,
            rotation_plane_supported: true,
            // Default box ~20 characters wide; overwritten with the entity's
            // own width when editing an existing MText.
            rect_width: (height * 20.0).max(1.0),
            keep_width: false,
            rect_width_draft: None,
            box_dragging: false,
            rect_height: "0".to_string(),
            annotative: false,
            column_type: 0,
            column_count: "2".to_string(),
            column_auto_height: false,
            column_flow_reversed: false,
            column_width: (height * 10.0).to_string(),
            column_gutter: (height * 5.0).to_string(),
            paragraph_first_indent: "0".to_string(),
            paragraph_left_indent: "0".to_string(),
            paragraph_right_indent: "0".to_string(),
            paragraph_space_before: "0".to_string(),
            paragraph_space_after: "0".to_string(),
            find_text: String::new(),
            replace_text: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            editing,
            screen_anchor: iced::Point::new(60.0, 90.0),
            original: None,
        }
    }

    /// What the `field` input shows: the value typed for the current
    /// selection, else the whole text's `base`.
    pub fn span_field<'a>(&'a self, field: &str, base: &'a str) -> &'a str {
        match &self.span_input {
            Some((sel, typed, value)) if Some(*sel) == self.sel && *typed == field => value,
            _ => base,
        }
    }

    pub fn height_value(&self) -> f64 {
        self.height
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|h| *h > 0.0)
            .unwrap_or(0.25)
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

    /// Fold the toolbar's global defaults (colour / oblique / char-spacing)
    /// onto every span that does not already override them. The object font
    /// and width are in the source itself.
    fn apply_globals(&self, doc: &mut MTextDocument) {
        let color = (self.color_aci != 256 && self.color_aci != 0)
            .then(|| MTextColor::Index(self.color_aci));
        let oblique = parse_non_default(&self.oblique, 0.0);
        let tracking = parse_non_default(&self.char_space, 0.0);
        for para in &mut doc.paragraphs {
            for span in &mut para.spans {
                let p = &mut span.properties;
                if p.color.is_none() && p.color_rgb.is_none() {
                    if let Some(ref c) = color {
                        p.color = Some(c.clone());
                    }
                }
                if p.oblique_angle.is_none() {
                    p.oblique_angle = oblique;
                }
                if p.tracking.is_none() {
                    p.tracking = tracking;
                }
            }
        }
    }

    /// The committed / previewed MText value: the structured body with the
    /// global toolbar defaults folded in, serialized by opencadcodec.
    fn folded_value(&self) -> String {
        let raw = self.content.text();
        // Without a toolbar default to fold in, the value is the source as
        // edited, byte for byte: nothing the user did not touch is rewritten.
        if matches!(self.color_aci, 0 | 256)
            && parse_non_default(&self.oblique, 0.0).is_none()
            && parse_non_default(&self.char_space, 0.0).is_none()
        {
            return raw;
        }
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
        mt.is_annotative = self.annotative;
        if let Some(color) = self.object_color {
            mt.common.color = color;
        }
        self.patch_rotation(&mut mt);
        let rectangle_height = self.rect_height.trim().parse::<f64>().unwrap_or(0.0);
        mt.rectangle_height = (rectangle_height > 0.0).then_some(rectangle_height);
        mt.column_data.column_type = self.column_type;
        if self.column_type == 0 {
            mt.column_data.column_count = 0;
            mt.column_data.heights.clear();
        } else {
            mt.column_data.column_count = crate::entities::text_support::clamp_mtext_column_count(
                self.column_count
                    .trim()
                    .parse::<i32>()
                    .ok()
                    .filter(|count| *count > 0)
                    .unwrap_or(1),
            );
            mt.column_data.auto_height = self.column_auto_height;
            mt.column_data.flow_reversed = self.column_flow_reversed;
            if let Ok(width) = self.column_width.trim().parse::<f64>() {
                if width > 0.0 {
                    mt.column_data.width = width;
                }
            }
            if let Ok(gutter) = self.column_gutter.trim().parse::<f64>() {
                if gutter >= 0.0 {
                    mt.column_data.gutter = gutter;
                }
            }
            // Manual columns of their own heights keep them while the column
            // settings and the box height stay as they were opened.
            let settings_kept = self.original.as_ref().is_some_and(|m| {
                m.column_data.column_type == mt.column_data.column_type
                    && m.column_data.column_count == mt.column_data.column_count
                    && m.column_data.auto_height == mt.column_data.auto_height
                    && m.rectangle_height == mt.rectangle_height
            });
            if !self.column_auto_height && rectangle_height > 0.0 && !settings_kept {
                mt.column_data.heights =
                    vec![rectangle_height; mt.column_data.column_count as usize];
            }
        }
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
#[cfg(test)]
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
                _ => para
                    .spans
                    .push(MTextSpan::new(ch.to_string(), props.clone())),
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
        .map(|item| {
            (
                (if item.is_rtl { item.xmax } else { item.xmin }),
                item.ymin,
                item.ymax,
            )
        })
        .or_else(|| {
            caret.checked_sub(1).and_then(|previous| {
                boxes.iter().find(|item| item.vis == previous).map(|item| {
                    (
                        (if item.is_rtl { item.xmin } else { item.xmax }),
                        item.ymin,
                        item.ymax,
                    )
                })
            })
        })
        .or_else(|| {
            boxes.first().map(|item| {
                (
                    (if item.is_rtl { item.xmax } else { item.xmin }),
                    item.ymin,
                    item.ymax,
                )
            })
        });
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

fn seed_mtext_state(state: &mut MTextEditorState, m: &MText) {
    state.attachment = m.attachment_point;
    state.line_spacing = m.line_spacing_factor as f32;
    let degrees = format!("{:.6}", m.rotation.to_degrees());
    state.rotation = degrees
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string();
    state.rect_width = m.rectangle_width.max(0.0);
    state.rect_height = m.rectangle_height.unwrap_or(0.0).max(0.0).to_string();
    state.annotative = m.is_annotative;
    state.column_type = m.column_data.column_type;
    state.column_count =
        crate::entities::text_support::clamp_mtext_column_count(m.column_data.column_count)
            .to_string();
    state.column_auto_height = m.column_data.auto_height;
    state.column_flow_reversed = m.column_data.flow_reversed;
    state.column_width = m.column_data.width.max(0.0).to_string();
    state.column_gutter = m.column_data.gutter.max(0.0).to_string();
    if !m.style.trim().is_empty() {
        state.style = m.style.clone();
    }
    state.original = Some(m.clone());
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
        template: Option<MText>,
    ) {
        if handle.is_some_and(|h| self.tabs[self.active_tab].scene.is_layer_locked(h)) {
            return;
        }
        let mut state = MTextEditorState::new(pos, initial, height, handle);
        if let Some(p) = self.tabs[self.active_tab]
            .scene
            .selection
            .borrow()
            .input
            .last_move_pos
        {
            state.screen_anchor = p;
        }
        // Seed attachment / line-spacing / box width from the entity being edited.
        let has_template = template.is_some();
        if let Some(h) = handle {
            match self.tabs[self.active_tab].scene.document.get_entity(h) {
                Some(EntityType::MText(m)) => {
                    seed_mtext_state(&mut state, m);
                }
                Some(EntityType::MultiLeader(ml)) => {
                    state.line_spacing = ml.context.line_spacing_factor as f32;
                    // A multileader without a width keeps none until one is set.
                    state.keep_width = ml.context.text_width > 0.0;
                    if state.keep_width {
                        state.rect_width = ml.context.text_width;
                    }
                }
                _ => {}
            }
        } else if let Some(m) = template {
            seed_mtext_state(&mut state, &m);
            state.keep_width = m.rectangle_width > 0.0;
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
        state.seed_object_values(&self.tabs[self.active_tab].scene.document);
        // A rotation typed for a new text turns about the working plane's Z,
        // supported only where that is the drawing's.
        if handle.is_none() && self.tabs[self.active_tab].editing_model_space() {
            state.rotation_plane_supported = self.tabs[self.active_tab]
                .ucs_xform()
                .working_plane()
                .z
                .abs_diff_eq(DVec3::Z, 1e-12);
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
        if handle.is_none() && !has_template {
            let initial_width = self
                .mtext_editor
                .as_ref()
                .map(|ed| super::view::overlay::MTEXT_EDITOR_WRITING_WIDTH / ed.preview_scale());
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
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let mut mt = ed.build_mtext();
        let editor_doc = raw_edit::editor_document(&ed.content.text());
        // A trailing empty paragraph (a fresh line after Enter) is dropped by the
        // layout's parser, so it emits no caret box there and the caret would be
        // invisible on that line. Inject a space into the PREVIEW value only
        // (never committed) so the layout lays the line out and the caret gets a
        // box on it.
        if editor_doc.paragraphs.len() > 1
            && editor_doc.paragraphs.last().is_some_and(|p| {
                p.spans
                    .iter()
                    .all(|s| s.text.is_empty() && s.stacking.is_none())
            })
        {
            let at = raw_adapter::SourceIndex::new(&mt.value).tail_boundary();
            mt.value.insert(at, ' ');
        }
        // Lay the preview out at the origin, not at the entity's real insertion.
        // The panel fits the strokes to its own box and never places them on the
        // drawing, so world position is irrelevant here — and a UTM-scale
        // insertion cast to f32 (glyph boxes) or split high/low (strokes, whose
        // low half this 2-D canvas drops) garbles every glyph through
        // catastrophic cancellation. Insertion is a pure translation, so zeroing
        // it keeps the layout's shape, wrapping and alignment identical.
        mt.insertion_point = Vector3::new(0.0, 0.0, 0.0);
        // The preview reads upright: the glyph boxes the caret and the
        // selection use are laid out unrotated, so the strokes must be too.
        mt.rotation = 0.0;
        mt.dwg_x_direction = None;
        mt.normal = Vector3::new(0.0, 0.0, 1.0);
        let entity = EntityType::MText(Box::new(mt.clone()));
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
            // single sync point, since every edit path ends in a rebuild. It
            // keeps a final empty paragraph, so Enter at the end keeps its line.
            ed.doc = editor_doc;
        }
    }

    pub(super) fn mtext_apply_span_number(&mut self, field: &'static str, value: String) {
        let number = value
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|&number| match field {
                "height" | "width" => number > 0.0,
                "oblique" => (-85.0..=85.0).contains(&number),
                "tracking" => number.is_finite(),
                _ => false,
            });
        let has_selection = self
            .mtext_editor
            .as_ref()
            .and_then(|editor| editor.sel)
            .is_some_and(|(start, end)| start < end);
        if field == "width" && !has_selection {
            self.mtext_object_width(value);
            return;
        }
        if let Some(ed) = self.mtext_editor.as_mut() {
            // The field shows what is typed, an empty or half-typed value
            // included, with or without a selection (#2194).
            if has_selection {
                ed.span_input = ed.sel.map(|sel| (sel, field, value));
            } else {
                ed.record_undo();
                match field {
                    "height" => ed.height = value,
                    "oblique" => ed.oblique = value,
                    "tracking" => ed.char_space = value,
                    _ => {}
                }
            }
        }
        match (has_selection, number) {
            (true, Some(number)) => self.raw_span_number(field, number),
            _ => self.rebuild_mtext_preview(),
        }
    }

    pub(super) fn mtext_apply_paragraph_number(&mut self, field: ParaNumber, value: String) {
        let Some(number) = value.trim().replace(',', ".").parse::<f64>().ok() else {
            return;
        };
        if !number.is_finite() {
            return;
        }
        if let Some(ed) = self.mtext_editor.as_mut() {
            match field {
                ParaNumber::FirstIndent => ed.paragraph_first_indent = value,
                ParaNumber::LeftIndent => ed.paragraph_left_indent = value,
                ParaNumber::RightIndent => ed.paragraph_right_indent = value,
                ParaNumber::SpaceBefore => ed.paragraph_space_before = value,
                ParaNumber::SpaceAfter => ed.paragraph_space_after = value,
            }
        }
        // Every paragraph the selection (or the caret) touches.
        self.raw_paragraph_number(field, number);
    }

    pub(super) fn mtext_stack_selection(&mut self) {
        self.mtext_stack();
    }

    /// Superscript (or subscript) the selection: a one-sided `^` stack.
    pub(super) fn mtext_script_selection(&mut self, superscript: bool) {
        self.mtext_script(if superscript {
            Script::Super
        } else {
            Script::Sub
        });
    }

    /// Remove the character formatting of the selection, or of the whole
    /// text without one. Paragraph settings, fractions and fields stay.
    pub(super) fn mtext_clear_formatting(&mut self) {
        self.raw_clear_formatting();
    }

    pub(super) fn mtext_find_next(&mut self) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            let needle: Vec<char> = ed.find_text.chars().collect();
            if needle.is_empty() {
                return;
            }
            let cells = doc_to_cells(&ed.doc);
            let chars: Vec<Option<char>> = cells
                .iter()
                .map(|cell| match cell {
                    Cell::Char(ch, _) => Some(*ch),
                    Cell::Break(..) => Some('\n'),
                    Cell::Stack { .. } => None,
                })
                .collect();
            if needle.len() > chars.len() {
                return;
            }
            let start = ed.caret.min(chars.len());
            let last_start = chars.len() - needle.len();
            let found = (start..=last_start)
                .chain(0..start.min(last_start + 1))
                .find(|index| {
                    chars[*index..*index + needle.len()]
                        .iter()
                        .zip(&needle)
                        .all(|(actual, expected)| actual == &Some(*expected))
                });
            if let Some(found) = found {
                ed.sel_anchor = found;
                ed.sel = Some((found, found + needle.len()));
                ed.caret = found + needle.len();
                ed.caret_blink_on = true;
            }
        }
    }

    pub(super) fn mtext_replace_next(&mut self) {
        let replacement = self
            .mtext_editor
            .as_ref()
            .map(|editor| editor.replace_text.clone())
            .unwrap_or_default();
        let matches = self.mtext_editor.as_ref().is_some_and(|editor| {
            let Some((start, end)) = editor.sel.filter(|(start, end)| start < end) else {
                return false;
            };
            let cells = doc_to_cells(&editor.doc);
            cells_to_doc(
                &ParagraphProperties::default(),
                &cells[start.min(cells.len())..end.min(cells.len())],
            )
            .to_plain_text()
                == editor.find_text
        });
        if matches {
            self.mtext_type(&replacement);
        }
        self.mtext_find_next();
    }

    pub(super) fn mtext_replace_all(&mut self) {
        self.raw_replace_all();
    }

    /// Toggle a character format over the preview selection, on the structured
    /// span properties. The stroke-font renderer has no true bold, so Bold
    /// switches the run to the heavier "Gothic" face; Italic applies a 15°
    /// oblique; underline / overline / strike flip the stroke flags; case
    /// rewrites the glyphs. All are true ON/OFF toggles.
    pub(super) fn mtext_apply_fmt(&mut self, kind: MTextFmt) {
        if matches!(kind, MTextFmt::Uppercase | MTextFmt::Lowercase) {
            self.raw_change_case(kind == MTextFmt::Uppercase);
            return;
        }
        let face = self.format_face();
        self.raw_format(kind, &face);
    }

    /// The face a bold run without its own font is stamped with, so the
    /// bold flag has a name to ride on and still draws the same font.
    pub(super) fn format_face(&self) -> String {
        let doc = &self.tabs[self.active_tab].scene.document;
        self.mtext_editor
            .as_ref()
            .map(|ed| {
                if !ed.font.trim().is_empty() {
                    ed.font.clone()
                } else {
                    // The family, not the style's bold / italic face of it.
                    let font =
                        crate::entities::text_support::resolve_text_style(&ed.style, doc).font_name;
                    crate::scene::text::sysfont::split_style(&font).0.to_string()
                }
            })
            .unwrap_or_default()
    }

    /// Set the alignment of every paragraph the selection (or caret) touches.
    pub(super) fn mtext_apply_align(&mut self, align: ParaAlign) {
        self.raw_align(align);
    }

    // ── Caret editing directly on the preview ───────────────────────────────

    /// Insert text (or replace the current selection) at the preview caret,
    /// patching the raw source in place (a line break goes in as `\P`).
    pub(super) fn mtext_type(&mut self, s: &str) {
        self.raw_type(s, true);
    }

    /// Delete the selection, or the visible character before the caret.
    pub(super) fn mtext_backspace(&mut self) {
        self.raw_delete(true);
    }

    /// Delete the selection, or the visible character at the caret.
    pub(super) fn mtext_delete(&mut self) {
        self.raw_delete(false);
    }

    /// Move the caret horizontally by `delta` visible characters.
    pub(super) fn mtext_caret_move(&mut self, delta: i32, extend_selection: bool) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.typing_format = Default::default();
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
    pub(super) fn mtext_caret_move_vertical(&mut self, direction: i8, extend_selection: bool) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            ed.typing_format = Default::default();
            let visible_count = doc_to_cells(&ed.doc).len();
            let caret = vertical_caret_target(&ed.glyph_boxes, ed.caret, visible_count, direction);
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
            ed.typing_format = Default::default();
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
                    let font =
                        crate::entities::text_support::resolve_text_style(&ed.style, doc)
                            .font_name;
                    crate::scene::text::sysfont::split_style(&font).0.to_string()
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
            self.raw_selection_font(&name);
        } else {
            self.mtext_object_font(font);
        }
    }

    /// Apply a colour from the shared Properties-style picker: to the selection
    /// (per-run) when there is one, else the whole text via the global colour.
    /// Closes the picker popup.
    pub(super) fn mtext_apply_color(&mut self, color: codec::types::Color) {
        use codec::types::Color as C;
        let has_sel = self
            .mtext_editor
            .as_ref()
            .and_then(|e| e.sel)
            .is_some_and(|(a, b)| a < b);
        if has_sel {
            self.raw_selection_color(color);
        } else if let Some(ed) = self.mtext_editor.as_mut().filter(|ed| {
            !self.command_mtext_input && !(ed.editing.is_some() && ed.original.is_none())
        }) {
            // No selection: the MTEXT's own colour, as the Properties panel
            // sets it; the text itself is not rewritten.
            ed.record_undo();
            ed.object_color = Some(color);
        } else if let Some(ed) = self.mtext_editor.as_mut() {
            // A multileader's text or command-line input has no colour of
            // its own: the whole-text colour applies (ACI only; a true
            // colour with no selection falls back to ByLayer).
            ed.record_undo();
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
            ed.typing_format = Default::default();
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

    /// Save & Close. See [`Self::commit_mtext_draft`].
    pub(super) fn mtext_commit(&mut self) -> bool {
        self.commit_mtext_draft(false)
    }

    /// Apply: save and keep editing the saved entity, the style-manager
    /// model. A new MText is created on the first Apply and updated in
    /// place after that, so repeated Apply never leaves duplicates.
    pub(super) fn mtext_apply(&mut self) {
        self.commit_mtext_draft(true);
    }

    /// Discard the editor without changing the drawing.
    pub(super) fn mtext_cancel(&mut self) {
        self.mtext_editor = None;
        self.command_mtext_input = false;
        self.pending_command_editor_text = None;
        self.reset_modal_geometry();
    }
}

#[cfg(test)]
mod data_tests;
#[cfg(test)]
mod rotation_tests;
#[cfg(test)]
mod stack_tests;

#[cfg(test)]
mod cell_tests {
    use super::*;
    use codec::entities::mtext_format::MTextParagraphAlignment;

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
        for s in [
            "hello",
            "a b c",
            "one\\Ptwo",
            "a\\Pb\\Pc",
            "trailing\\P",
            "a  b",
            "\\S1/2;",
        ] {
            let once = rt(s);
            let twice = rt(&once);
            assert_eq!(
                once, twice,
                "not idempotent for {s:?}: {once:?} vs {twice:?}"
            );
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
    }

    #[test]
    fn stacking_is_atomic() {
        let cells = cells_of("\\S1/2;");
        assert_eq!(cells.len(), 3);
        assert!(matches!(cells[0], Cell::Stack { head: true, .. }));
        assert!(matches!(cells[1], Cell::Stack { head: false, .. }));
        assert!(matches!(cells[2], Cell::Stack { head: false, .. }));
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
        use codec::entities::mtext_format::{MTextColor, MTextFont};
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
    // (replacing the old hand-built `\C;\Q;…` prefix), and survive
    // serialization. The object font and width are in the source instead.
    #[test]
    fn global_defaults_fold_into_spans() {
        let mut ed = MTextEditorState::new(DVec3::ZERO, "hello world", 2.5, None);
        ed.color_aci = 1; // red
        ed.oblique = "15".to_string();
        let doc = parse_mtext(&ed.folded_value(), true);
        assert_eq!(doc.to_plain_text(), "hello world");
        let p = &doc.paragraphs[0].spans[0].properties;
        assert_eq!(p.color, Some(MTextColor::Index(1)));
        assert_eq!(p.oblique_angle, Some(15.0));
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
        assert_eq!(
            doc_to_cells(&d).len(),
            3,
            "trailing break must survive rebuild"
        );
    }
}

