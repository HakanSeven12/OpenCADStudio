use super::super::Message;
mod mtext_menu;
#[cfg(test)]
mod mtext_tests;
mod mtext_toolbar;
use crate::t;
use crate::ui::popup::context_menu::{
    ContextMenu, MenuIcon, MenuItem, MenuRow, MENU_PAD_TOP, MENU_ROW_H,
};
use crate::ui::popup::quad_menu::{QuadMsg, QuadPanel, QuadUi};
use iced::advanced::layout;
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget;
use iced::advanced::{Layout, Shell, Widget};
use iced::widget::{
    button, checkbox, column, container, mouse_area, row, scrollable, stack, text, text_input,
    Space,
};
use iced::{
    Background, Border, Color, Element, Event, Fill, Length, Rectangle, Size, Theme, Vector,
};

pub(super) fn position_canvas_overlay<'a>(
    anchor: iced::Point,
    panel: Element<'a, Message>,
) -> Element<'a, Message> {
    iced::widget::pin(iced::widget::opaque(panel))
        .position(iced::Point::new(anchor.x.max(0.0), anchor.y.max(0.0)))
        .into()
}

// ── In-place MText editor overlay ───────────────────────────────────────────

/// Widget id for the MText editor's text area (focused when Edit mode opens).
pub(in crate::app) const MTEXT_TEXT_ID: &str = "mtext_editor_text";

/// Widget id for the in-place TEXT editor's input (focused when it opens).
pub(in crate::app) const TEXT_INLINE_ID: &str = "text_inline_input";

/// In-place single-line TEXT editor: a plain text-entry box (no formatting
/// toolbar), anchored at the insertion-point click. Enter commits; Esc cancels.
pub(super) fn text_inline_overlay(
    ed: &super::super::text_inline::TextInlineState,
    canvas: (f32, f32),
) -> Element<'_, Message> {
    let field = text_input(t!("Text").as_ref(), &ed.value)
        .id(iced::widget::Id::new(TEXT_INLINE_ID))
        .on_input(Message::TextInlineInput)
        .on_submit(Message::TextInlineOk)
        .padding(6)
        .size(13)
        .width(iced::Length::Fixed(240.0));

    let panel = container(field)
        .style(move |theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 5.0.into(),
                },
                ..Default::default()
            }
        })
        .padding(4);

    // Keep the box on-screen so its field stays clickable at the edges.
    const PANEL_W: f32 = 240.0 + 20.0;
    const PANEL_H: f32 = 46.0;
    let (cw, ch) = canvas;
    let anchor = iced::Point::new(
        (ed.screen_anchor.x - 6.0).clamp(0.0, (cw - PANEL_W).max(0.0)),
        (ed.screen_anchor.y - 18.0).clamp(0.0, (ch - PANEL_H).max(0.0)),
    );
    position_canvas_overlay(anchor, panel.into())
}

/// (label, ACI). 256 = ByLayer.
/// Canvas program that renders the tessellated MText strokes inside the
/// editor's own preview area (never on the drawing). Strokes lie in the
/// world XY plane; the program fits + vertically flips them into the box.
pub(in crate::app) const MTEXT_PREVIEW_PAD: f32 = 12.0;
pub(in crate::app) const MTEXT_PREVIEW_EM_PX: f32 = 15.0;
// Used only until the shared modal sensor reports the first real layout width.
const MTEXT_EDITOR_FALLBACK_WIDTH: f32 = 660.0;
pub(in crate::app) const MTEXT_EDITOR_WRITING_WIDTH: f32 =
    MTEXT_EDITOR_FALLBACK_WIDTH - 2.0 * MTEXT_PREVIEW_PAD;
/// The editor never shrinks below its two toolbar rows and a few lines of
/// text; a smaller window still caps it.
const MTEXT_EDITOR_MIN_SIZE: Size = Size::new(900.0, 370.0);

struct MTextPreview {
    /// Disconnected polylines as (x, y) world points + colour (NaN-split done).
    segments: Vec<(Vec<(f32, f32)>, Color, f32)>,
    /// Per-visible-character boxes (world frame) for click-to-select.
    boxes: Vec<crate::entities::text_support::GlyphBox>,
    /// Current selection as a visible-char range.
    sel: Option<(usize, usize)>,
    /// Caret position as a visible-char offset.
    caret: usize,
    /// Whether the caret is in its visible blink phase.
    caret_on: bool,
    /// World-space min corner (bbox) and pixels-per-world-unit scale.
    minx: f32,
    miny: f32,
    scale: f32,
    content_h: f32,
    /// The box-width ruler above the text; absent on a turned preview.
    ruler: Option<BoxRuler>,
    /// The text's rotation (cos, sin). Strokes and glyph boxes are laid out
    /// upright; the preview turns them only when drawing, and turns pointer
    /// positions back, so caret and selection stay in the layout frame.
    rotation: (f32, f32),
}

/// Height of the box-width ruler band at the top of the preview.
pub(in crate::app) const MTEXT_RULER_H: f32 = 18.0;
/// How near (px) to the handle a press grabs it.
const RULER_GRAB_PX: f32 = 7.0;

/// The text box along the ruler, in layout units, and its draggable edge.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BoxRuler {
    left: f32,
    right: f32,
    /// Width change per unit the handle moves: 1 left-attached (right edge),
    /// 2 centred (both edges), -1 right-attached (the left edge moves).
    growth: f32,
}

impl BoxRuler {
    fn handle(&self) -> f32 {
        if self.growth < 0.0 {
            self.left
        } else {
            self.right
        }
    }
}

/// The full height of the text line a glyph box sits on: the box spans the
/// cap height only, so descenders, accents and the line gap are added, and
/// a line never reaches into its neighbour's band.
fn line_band(
    boxes: &[crate::entities::text_support::GlyphBox],
    b: &crate::entities::text_support::GlyphBox,
) -> (f32, f32) {
    let h = (b.ymax - b.ymin).max(f32::EPSILON);
    let mut low = b.ymin - 0.35 * h;
    let mut high = b.ymax + 0.2 * h;
    for other in boxes {
        if other.ymax <= b.ymin {
            low = low.max((other.ymax + b.ymin) * 0.5);
        } else if other.ymin >= b.ymax {
            high = high.min((other.ymin + b.ymax) * 0.5);
        }
    }
    (low, high)
}

/// Rotate a layout-frame point by the text's rotation.
fn turn((cos, sin): (f32, f32), x: f32, y: f32) -> (f32, f32) {
    (x * cos - y * sin, x * sin + y * cos)
}

impl MTextPreview {
    /// Screen point of a layout-frame point: turned by the rotation, drawn
    /// at the real size, Y flipped (world up → screen down).
    fn to_screen(&self, x: f32, y: f32) -> iced::Point {
        let (x, y) = turn(self.rotation, x, y);
        iced::Point::new(
            MTEXT_PREVIEW_PAD + (x - self.minx) * self.scale,
            self.content_h - (MTEXT_PREVIEW_PAD + (y - self.miny) * self.scale),
        )
    }

    /// Whether a point (bounds-local px) grabs the ruler handle.
    fn on_handle(&self, p: iced::Point) -> bool {
        p.y <= MTEXT_RULER_H
            && self
                .ruler
                .is_some_and(|r| (p.x - self.to_screen(r.handle(), 0.0).x).abs() <= RULER_GRAB_PX)
    }

    /// The width a drag reaches after moving `dx` px from where it started.
    fn dragged_width(&self, start_width: f32, dx: f32) -> Option<f64> {
        let ruler = self.ruler?;
        let width = start_width + ruler.growth * dx / self.scale;
        Some(f64::from(width.max(self.scale.recip())))
    }

    /// Visible-char offset (0..=N) nearest the cursor point (bounds-local px).
    fn offset_at(&self, p: iced::Point) -> usize {
        if self.boxes.is_empty() {
            return 0;
        }
        let rx = self.minx + (p.x - MTEXT_PREVIEW_PAD) / self.scale;
        let ry = self.miny + (self.content_h - p.y - MTEXT_PREVIEW_PAD) / self.scale;
        let (wx, wy) = turn((self.rotation.0, -self.rotation.1), rx, ry);
        let mut best = 0usize;
        let mut best_d = f32::MAX;
        for b in &self.boxes {
            let dx = if wx < b.xmin {
                b.xmin - wx
            } else if wx > b.xmax {
                wx - b.xmax
            } else {
                0.0
            };
            let dy = if wy < b.ymin {
                b.ymin - wy
            } else if wy > b.ymax {
                wy - b.ymax
            } else {
                0.0
            };
            let d = dy * 1000.0 + dx; // prefer the correct line first
            if d < best_d {
                best_d = d;
                if b.is_rtl {
                    if wx < (b.xmin + b.xmax) * 0.5 {
                        best = b.vis + 1;
                    } else {
                        best = b.vis;
                    }
                } else {
                    best = b.vis;
                    // After the glyph centre → caret sits after this char.
                    if wx > (b.xmin + b.xmax) * 0.5 {
                        best = b.vis + 1;
                    }
                }
            }
        }
        best
    }
}

#[derive(Default)]
struct MTextPreviewState {
    dragging: bool,
    /// A ruler drag: where it started (px) and the box width then.
    width_drag: Option<(f32, f32)>,
}

impl iced::widget::canvas::Program<Message> for MTextPreview {
    type State = MTextPreviewState;

    fn update(
        &self,
        state: &mut MTextPreviewState,
        event: &iced::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<iced::widget::canvas::Action<Message>> {
        use iced::mouse::{Button, Event as Me};
        use iced::widget::canvas::Action;
        use iced::Event;
        match event {
            Event::Mouse(Me::ButtonPressed(Button::Left)) => {
                if let Some(p) = cursor.position_in(bounds) {
                    if let Some(ruler) = self.ruler.filter(|_| self.on_handle(p)) {
                        state.width_drag = Some((p.x, ruler.right - ruler.left));
                        return Some(Action::capture());
                    }
                    if p.y <= MTEXT_RULER_H && self.ruler.is_some() {
                        // The ruler band is not text: no caret move.
                        return Some(Action::capture());
                    }
                    state.dragging = true;
                    let off = self.offset_at(p);
                    return Some(Action::publish(Message::MTextSelStart(off)).and_capture());
                }
            }
            Event::Mouse(Me::CursorMoved { .. }) => {
                if let Some((start, width)) = state.width_drag {
                    let x = cursor.position_from(bounds.position())?.x;
                    let width = self.dragged_width(width, x - start)?;
                    return Some(Action::publish(Message::MTextBoxDrag(Some(width))).and_capture());
                }
                if state.dragging {
                    if let Some(p) = cursor.position_in(bounds) {
                        let off = self.offset_at(p);
                        return Some(Action::publish(Message::MTextSelTo(off)));
                    }
                }
            }
            Event::Mouse(Me::ButtonReleased(Button::Left)) => {
                if state.width_drag.take().is_some() {
                    return Some(Action::publish(Message::MTextBoxDrag(None)).and_capture());
                }
                if state.dragging {
                    state.dragging = false;
                    return Some(Action::capture());
                }
            }
            _ => {}
        }
        None
    }

    fn mouse_interaction(
        &self,
        state: &MTextPreviewState,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        let grabbing = state.width_drag.is_some()
            || cursor
                .position_in(bounds)
                .is_some_and(|p| self.on_handle(p));
        if grabbing {
            iced::mouse::Interaction::ResizingHorizontally
        } else {
            iced::mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _state: &MTextPreviewState,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path, Stroke};
        let mut frame = Frame::new(renderer, bounds.size());
        let map = |x: f32, y: f32| self.to_screen(x, y);
        // Selection highlight behind the glyphs, over the whole line height
        // (descenders and accents included), turned with the text.
        if let Some((a, b)) = self.sel {
            for bx in &self.boxes {
                if bx.vis >= a && bx.vis < b {
                    let (low, high) = line_band(&self.boxes, bx);
                    let quad = Path::new(|p| {
                        p.move_to(map(bx.xmin, low));
                        p.line_to(map(bx.xmax, low));
                        p.line_to(map(bx.xmax, high));
                        p.line_to(map(bx.xmin, high));
                        p.close();
                    });
                    frame.fill(&quad, theme.palette().primary.base.color.scale_alpha(0.45));
                }
            }
        }
        for (seg, col, width) in &self.segments {
            if seg.len() < 2 {
                continue;
            }
            let path = Path::new(|p| {
                p.move_to(map(seg[0].0, seg[0].1));
                for &(x, y) in &seg[1..] {
                    p.line_to(map(x, y));
                }
            });
            let col = readable_on(*col, theme.palette().background.base);
            frame.stroke(&path, Stroke::default().with_color(col).with_width(*width));
        }
        if let Some(ruler) = self.ruler {
            // The box width: a rule between the box edges with end ticks and
            // a handle on the edge that moves.
            let color = theme.palette().primary.base.color;
            let mid = MTEXT_RULER_H * 0.5;
            let (l, r) = (map(ruler.left, 0.0).x, map(ruler.right, 0.0).x);
            let rule = Path::new(|p| {
                p.move_to(iced::Point::new(l, mid));
                p.line_to(iced::Point::new(r, mid));
                for x in [l, r] {
                    p.move_to(iced::Point::new(x, 4.0));
                    p.line_to(iced::Point::new(x, MTEXT_RULER_H - 4.0));
                }
            });
            frame.stroke(&rule, Stroke::default().with_color(color).with_width(1.0));
            let x = map(ruler.handle(), 0.0).x;
            frame.fill_rectangle(
                iced::Point::new(x - 3.0, 2.0),
                iced::Size::new(6.0, MTEXT_RULER_H - 4.0),
                color,
            );
        }
        // Caret — a vertical bar at the caret's glyph boundary, shown when the
        // selection is empty (a plain text cursor).
        // Caret is shown only when the selection is empty and the blink is in
        // its visible phase.
        let collapsed = self.caret_on && self.sel.map(|(a, b)| a == b).unwrap_or(true);
        if collapsed && self.boxes.is_empty() {
            // Empty text: show a caret at the top-left so the user can type.
            // Its height matches the preview's constant em (see EM_PX) so the
            // empty-editor caret isn't tiny.
            let top = MTEXT_PREVIEW_PAD + self.ruler.map_or(0.0, |_| MTEXT_RULER_H);
            let path = Path::new(|p| {
                p.move_to(iced::Point::new(MTEXT_PREVIEW_PAD, top));
                p.line_to(iced::Point::new(
                    MTEXT_PREVIEW_PAD,
                    (top + 40.0).min(self.content_h),
                ));
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(theme.palette().warning.base.color)
                    .with_width(1.5),
            );
        } else if collapsed {
            let bar = if let Some(b) = self.boxes.iter().find(|b| b.vis == self.caret) {
                let cx = if b.is_rtl { b.xmax } else { b.xmin };
                Some((cx, b.ymin, b.ymax))
            } else if self.caret > 0 {
                self.boxes
                    .iter()
                    .find(|b| b.vis == self.caret - 1)
                    .map(|b| {
                        let cx = if b.is_rtl { b.xmin } else { b.xmax };
                        (cx, b.ymin, b.ymax)
                    })
            } else {
                self.boxes.first().map(|b| {
                    let cx = if b.is_rtl { b.xmax } else { b.xmin };
                    (cx, b.ymin, b.ymax)
                })
            };
            if let Some((cx, y0, y1)) = bar {
                let p0 = map(cx, y0);
                let p1 = map(cx, y1);
                let path = Path::new(|p| {
                    p.move_to(p0);
                    p.line_to(p1);
                });
                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_color(theme.palette().warning.base.color)
                        .with_width(1.5),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

/// Split every preview WireModel into finite (x, y) polyline runs, each
/// carrying its wire's colour (so inline `\C` / the colour dropdown shows) and a
/// stroke width (bold runs carry a wider pen via `line_weight_px`).
/// A text colour too close to the editor background (a light grey layer on
/// the light theme's white) pulled toward the theme's text colour, keeping
/// some of its own hue.
fn readable_on(col: Color, background: iced::theme::palette::Pair) -> Color {
    let luminance = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if (luminance(col) - luminance(background.color)).abs() >= 0.35 {
        return col;
    }
    let text = background.text;
    let mix = |a: f32, b: f32| a * 0.3 + b * 0.7;
    Color {
        r: mix(col.r, text.r),
        g: mix(col.g, text.g),
        b: mix(col.b, text.b),
        a: col.a,
    }
}

fn mtext_preview_segments(
    ed: &super::super::mtext_editor::MTextEditorState,
) -> Vec<(Vec<(f32, f32)>, Color, f32)> {
    let mut out: Vec<(Vec<(f32, f32)>, Color, f32)> = Vec::new();
    for w in &ed.preview_wires {
        let col = Color {
            r: w.color[0],
            g: w.color[1],
            b: w.color[2],
            a: 1.0,
        };
        // Bold text wires bake a wider pen (line_weight_px ~2.4); draw them thick.
        let width = if w.line_weight_px > 1.5 { 2.6 } else { 1.4 };
        let mut run: Vec<(f32, f32)> = Vec::new();
        for p in &w.points {
            if p[0].is_finite() && p[1].is_finite() {
                run.push((p[0], p[1]));
            } else if !run.is_empty() {
                out.push((std::mem::take(&mut run), col, width));
            }
        }
        if !run.is_empty() {
            out.push((run, col, width));
        }
    }
    out
}

pub(super) fn mtext_editor_overlay<'a>(
    ed: &'a super::super::mtext_editor::MTextEditorState,
    styles: Vec<String>,
    modal_offset: iced::Vector,
    modal_resize: iced::Vector,
    modal_content_size: Option<iced::Size>,
) -> Element<'a, Message> {
    let writing_area_px = modal_content_size
        .map(|size| size.width - 2.0 * MTEXT_PREVIEW_PAD)
        .unwrap_or(MTEXT_EDITOR_WRITING_WIDTH)
        .max(80.0);
    let measurement = mtext_editor_content(
        ed,
        &styles,
        crate::ui::modal::ModalSizing::INTRINSIC,
        (writing_area_px * 0.5).max(1.0),
    );
    let content = mtext_editor_content(
        ed,
        &styles,
        crate::ui::modal::ModalSizing::FILL,
        (writing_area_px * 0.5).max(1.0),
    );
    let content = crate::ui::modal::intrinsic_with_min(
        measurement,
        content,
        iced::Size::INFINITE,
        modal_resize,
        MTEXT_EDITOR_MIN_SIZE,
    );

    crate::ui::modal::modal(
        iced::widget::Space::new().width(Fill).height(Fill),
        t!("Text Editor"),
        content,
        Message::MTextOk,
        modal_offset,
        crate::ui::modal::ModalOptions::STANDARD,
    )
}

/// The right-click menu of a first-row field with a value of its own. One
/// `mouse_area` row: a button inside a `ContextMenu` overlay loses its press
/// state on every rebuild and never publishes (see `xref_manager::menu_row`).
fn reset_menu(field: &'static str, value: &str) -> Element<'static, Message> {
    let row = iced::widget::mouse_area(
        container(text(crate::tf!("Reset to style ({value})")).size(12)).padding([5, 12]),
    )
    .on_press(Message::MTextResetToStyle(field))
    .interaction(iced::mouse::Interaction::Pointer);
    container(row)
        .style(container::bordered_box)
        .padding(2)
        .into()
}

fn mtext_editor_content<'a>(
    ed: &'a super::super::mtext_editor::MTextEditorState,
    styles: &[String],
    sizing: crate::ui::modal::ModalSizing,
    intrinsic_preview_height: f32,
) -> Element<'a, Message> {
    let width = sizing.width;
    let height = sizing.height;
    let preview_height = if matches!(height, iced::Length::Fill) {
        iced::Length::Fill
    } else {
        iced::Length::Fixed(intrinsic_preview_height)
    };
    use iced::widget::canvas;

    let small_input = |placeholder: &'static str,
                       val: &str,
                       on: fn(String) -> Message,
                       w: f32|
     -> Element<'static, Message> {
        text_input(placeholder, val)
            .on_input(on)
            .width(iced::Length::Fixed(w))
            .padding(3)
            .size(12)
            .into()
    };

    // ── Row 1: the object's own values, each labelled ──────────────────────
    // A label above its control; font, height, width and oblique also say,
    // on a line of their own under the label, where their value comes from:
    // the style (green), the text's own (red), or the style with overrides
    // (orange).
    let labelled = |name: std::borrow::Cow<'static, str>,
                    origin: Option<super::super::mtext_editor::ValueOrigin>,
                    control: Element<'a, Message>|
     -> Element<'a, Message> {
        use super::super::mtext_editor::ValueOrigin;
        let mark: Element<'a, Message> = match origin {
            Some(origin) => {
                let (label, accent): (_, fn(&Theme) -> iced::Color) = match origin {
                    ValueOrigin::Style => (crate::tr!("mtext-object-origin", "style"), |theme| {
                        theme.palette().success.base.color
                    }),
                    ValueOrigin::Custom => {
                        (crate::tr!("mtext-object-origin", "custom"), |theme| {
                            theme.palette().danger.base.color
                        })
                    }
                    ValueOrigin::Mixed => (crate::tr!("mtext-object-origin", "mixed"), |theme| {
                        theme.palette().warning.base.color
                    }),
                };
                text(label)
                    .size(10)
                    .font(iced::Font {
                        style: iced::font::Style::Italic,
                        ..iced::Font::DEFAULT
                    })
                    .style(move |theme: &Theme| {
                        // Readable on the editor's panel, lighter than the
                        // base in a dark theme and darker in a light one.
                        let background = theme.palette().background;
                        text::Style {
                            color: Some(crate::ui::style::common::readable_accent(
                                accent(theme),
                                background.weak.color,
                                background.base.text,
                            )),
                        }
                    })
                    .into()
            }
            None => iced::widget::Space::new().into(),
        };
        // The mark keeps its line when there is none, and every control sits
        // in a band of one height: the labels, the marks and the controls
        // each share one line whatever the control's own height.
        let mark = container(mark).height(13);
        let control = container(control).height(30).center_y(30);
        iced::widget::column![text(name).size(11), mark, control]
            .spacing(0)
            .into()
    };
    let mut style_opts: Vec<String> = if styles.is_empty() {
        vec!["Standard".to_string()]
    } else {
        styles.to_vec()
    };
    if !style_opts.contains(&ed.style) {
        style_opts.push(ed.style.clone());
    }
    let style_pl = crate::ui::dropdown_list::pick_list(Some(ed.style.clone()), style_opts, |value| {
        value.to_string()
    })
    .on_select(Message::MTextStyle)
    .text_size(11)
    .width(iced::Length::Fixed(96.0));
    let annotative = checkbox(ed.annotative)
        .on_toggle(Message::MTextAnnotative)
        .size(14);
    // Without a font of its own the text draws with the style's, which the
    // field shows (the mark under the label says "By style").
    let font = ed.font_field();
    let selected = match &ed.object_style.font {
        Some(style_font) if font.trim().is_empty() => style_font.clone(),
        _ => crate::ui::font_picker::FontChoice::of_name(&font),
    };
    let file = match &selected {
        crate::ui::font_picker::FontChoice::Stroke(file) => file.clone(),
        _ => font,
    };
    let font_pl = crate::ui::font_picker::view(crate::ui::font_picker::FontPicker {
        state: &ed.font_picker,
        selected,
        file,
        width: iced::Length::Fixed(150.0),
        read_only: false,
        on_event: Message::MTextFontPicker,
    });
    // Same colour picker as the Properties panel (named swatches + "More…" full
    // palette), applied to the selection or the whole text.
    let color_pl = iced::widget::container(crate::ui::color_select::color_selector(
        ed.whole_text_color(),
        ed.color_picker_open,
        crate::ui::color_select::ColorExtras {
            by_layer: true,
            by_block: false,
            ..Default::default()
        },
        Message::MTextColorChanged,
        Message::MTextColorPickerToggle,
        Message::OpenColorWindow(crate::app::ColorPickTarget::MText, ed.whole_text_color()),
    ))
    .width(iced::Length::Fixed(150.0));

    // A value of the text's own takes "Reset to style" on a right click.
    let resettable = |field: &'static str,
                      origin: Option<super::super::mtext_editor::ValueOrigin>,
                      cell: Element<'a, Message>|
     -> Element<'a, Message> {
        if origin != Some(super::super::mtext_editor::ValueOrigin::Custom) {
            return cell;
        }
        let value = ed.style_value(field);
        iced_aw::ContextMenu::new(cell, move || reset_menu(field, &value)).into()
    };
    let font_origin = Some(ed.value_origin("font"));
    let height_origin = ed.height_origin();
    let width_origin = Some(ed.value_origin("width"));
    let oblique_origin = Some(ed.value_origin("oblique"));
    let row1 = row![
        labelled(
            t!("Style"),
            None,
            row![style_pl, mtext_menu::style_menu(ed)]
                .spacing(2)
                .align_y(iced::Alignment::Center)
                .into()
        ),
        resettable(
            "font",
            font_origin,
            labelled(t!("Font"), font_origin, font_pl)
        ),
        resettable(
            "height",
            height_origin,
            labelled(
                t!("Height"),
                height_origin,
                small_input(
                    "2.5",
                    ed.span_field("height", &ed.height),
                    Message::MTextHeight,
                    64.0
                )
            )
        ),
        resettable(
            "width",
            width_origin,
            labelled(
                t!("Width factor"),
                width_origin,
                small_input(
                    "1",
                    ed.span_field("width", &ed.width),
                    Message::MTextWidth,
                    64.0
                )
            )
        ),
        resettable(
            "oblique",
            oblique_origin,
            labelled(
                crate::tr!("mtext-object-origin", "oblique").into(),
                oblique_origin,
                row![
                    small_input(
                        "0",
                        ed.span_field("oblique", &ed.oblique),
                        Message::MTextOblique,
                        52.0
                    ),
                    text("°").size(12)
                ]
                .spacing(3)
                .align_y(iced::Alignment::Center)
                .into()
            )
        ),
        labelled(
            t!("Rotation"),
            None,
            row![
                small_input("0", &ed.rotation, Message::MTextRotation, 52.0),
                text("°").size(12)
            ]
            .spacing(3)
            .align_y(iced::Alignment::Center)
            .into()
        ),
        labelled(t!("Color"), None, color_pl.into()),
        labelled(
            "".into(),
            None,
            row![annotative, text(t!("Annotative")).size(11)]
                .spacing(3)
                .align_y(iced::Alignment::Center)
                .into()
        ),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Start)
    .width(width);

    // ── Row 2: character formatting, undo, alignment and the menus ─────────
    // Below its full width the row scrolls sideways; the thin bar sits
    // under the controls, never over them.
    let row2 = iced::widget::scrollable(mtext_toolbar::format_row(ed))
        .id("mtext-toolbar-2")
        .direction(iced::widget::scrollable::Direction::Horizontal(
            iced::widget::scrollable::Scrollbar::new()
                .width(4)
                .scroller_width(4)
                .spacing(2),
        ))
        .width(width);

    let preview_scale = ed.preview_scale();

    // ── Body: the rendered preview (the editor is preview-only). It fills the
    // space left by the toolbars, so the resizable modal's extra height flows
    // into the text area. ─────────────────────────────────────────────────
    let body: Element<'a, Message> = if !matches!(height, iced::Length::Fill) {
        // The preview is a flexible scroll viewport. Measuring its real canvas
        // would make long/wide MText dictate the dialog size, so the intrinsic
        // pass uses an empty viewport proxy. The toolbars alone determine its
        // width; the preview height follows that measured writing width.
        container(Space::new())
            .style(move |theme: &Theme| {
                let palette = theme.palette();
                container::Style {
                    background: Some(Background::Color(palette.background.base.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }
            })
            .padding(2)
            .width(width)
            .height(preview_height)
            .into()
    } else {
        let segments = mtext_preview_segments(ed);
        // The preview shows the text at the angle it will have in the drawing.
        let angle = ed.rotation_input_value().unwrap_or(0.0) as f32;
        let rotation = if angle == 0.0 {
            (1.0, 0.0)
        } else {
            (angle.cos(), angle.sin())
        };
        let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut extend = |x: f32, y: f32| {
            let (x, y) = turn(rotation, x, y);
            minx = minx.min(x);
            miny = miny.min(y);
            maxx = maxx.max(x);
            maxy = maxy.max(y);
        };
        for (seg, _, _) in &segments {
            for &(x, y) in seg {
                extend(x, y);
            }
        }
        // Include glyph boxes so all-whitespace / box-only lines still anchor
        // the transform (hit-testing relies on minx/miny).
        for b in &ed.glyph_boxes {
            for (x, y) in [
                (b.xmin, b.ymin),
                (b.xmax, b.ymin),
                (b.xmax, b.ymax),
                (b.xmin, b.ymax),
            ] {
                extend(x, y);
            }
        }
        let scale = preview_scale;
        // The ruler reads along the layout's x axis, so a turned preview has
        // none (the status bar's field still sets the width). The box grows
        // from its attachment: rightwards, both ways or leftwards.
        let ruler = (rotation == (1.0, 0.0)).then(|| {
            let width = ed.rect_width as f32;
            let growth = ed.box_growth();
            let (left, right) = if growth == 2.0 {
                (-width * 0.5, width * 0.5)
            } else if growth < 0.0 {
                (-width, 0.0)
            } else {
                (0.0, width)
            };
            BoxRuler {
                left,
                right,
                growth,
            }
        });
        if let Some(ruler) = ruler {
            if maxx < minx {
                (minx, maxx, miny, maxy) = (ruler.left, ruler.right, 0.0, 0.0);
            }
            minx = minx.min(ruler.left);
            maxx = maxx.max(ruler.right);
        }
        let band = ruler.map_or(0.0, |_| MTEXT_RULER_H);
        let content_h = band
            + if maxx >= minx {
                ((maxy - miny) * scale + 2.0 * MTEXT_PREVIEW_PAD).max(40.0)
            } else {
                40.0
            };
        // Multi-column MTEXT lays its columns out side by side, so the content
        // can be wider than the editor; size the canvas to the real content
        // width and let the scroll area pan horizontally to reach later columns.
        let content_w = if maxx >= minx {
            ((maxx - minx).max(ed.rect_width as f32) * scale + 2.0 * MTEXT_PREVIEW_PAD).max(40.0)
        } else {
            ed.rect_width as f32 * scale + 2.0 * MTEXT_PREVIEW_PAD
        };
        let prog = MTextPreview {
            segments,
            boxes: ed.glyph_boxes.clone(),
            sel: ed.sel,
            caret: ed.caret,
            caret_on: ed.caret_blink_on,
            minx,
            miny,
            scale,
            content_h,
            ruler,
            rotation,
        };
        let cv = canvas(prog)
            .width(iced::Length::Fixed(content_w))
            .height(iced::Length::Fixed(content_h));
        // The preview fills the space the toolbars leave; it scrolls in BOTH
        // directions so a taller-than-view text scrolls vertically and a
        // wider-than-view multi-column text scrolls horizontally.
        use iced::widget::scrollable::{Direction, Scrollbar};
        // Typing into the preview goes through the platform input method, so
        // Chinese, Japanese and Korean text can be entered (#2231).
        let cv = crate::ui::ime_zone::ime_zone(cv, Message::CommandAppendChar);
        container(
            iced::widget::scrollable(cv)
                .direction(Direction::Both {
                    vertical: Scrollbar::default(),
                    horizontal: Scrollbar::default(),
                })
                .width(width)
                .height(preview_height),
        )
        .style(move |theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.background.base.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        })
        .padding(2)
        .id("mtext-editor-viewport")
        .width(width)
        .height(preview_height)
        .into()
    };

    container(
        column![row1, row2, body, mtext_toolbar::status_bar(ed, width)]
            .spacing(6)
            .width(width)
            .height(height),
    )
    .width(width)
    .height(height)
    .into()
}

/// Position a cursor-anchored panel inside the drawing's safe rectangle. The
/// panel is laid out first, so edge flipping uses its actual translated size
/// instead of estimates. `bottom_inset` reserves overlaid controls such as the
/// command line.
///
/// `offset` shifts the preferred placement from the anchor before the fit /
/// flip logic runs (e.g. to put a specific row of a menu under the pointer).
/// When the shifted panel does not fit it flips to the other side of the
/// *original* anchor, so the pointer never ends up over an unintended row.
pub(super) fn position_canvas_overlay_clamped<'a>(
    anchor: iced::Point,
    bottom_inset: f32,
    offset: Vector,
    panel: Element<'a, Message>,
) -> Element<'a, Message> {
    Element::new(ClampedPin {
        content: iced::widget::opaque(panel),
        anchor,
        bottom_inset: bottom_inset.max(0.0),
        gap: 0.0,
        offset,
        passive: false,
    })
}

/// Keep a floating panel close to the cursor without letting it cover the
/// pointer or leave the drawing's safe rectangle.
pub(super) fn position_canvas_overlay_near_cursor<'a>(
    cursor: iced::Point,
    bottom_inset: f32,
    panel: Element<'a, Message>,
) -> Element<'a, Message> {
    Element::new(ClampedPin {
        content: iced::widget::opaque(panel),
        anchor: cursor,
        bottom_inset: bottom_inset.max(0.0),
        gap: 12.0,
        offset: Vector::ZERO,
        passive: false,
    })
}

/// Native parameter help stays within the viewport and passes input through.
pub(super) fn grip_tip_overlay<'a>(
    anchor: iced::Point,
    bottom_inset: f32,
    value: String,
) -> Element<'a, Message> {
    let panel = container(
        text(value)
            .size(12)
            .shaping(iced::advanced::text::Shaping::Advanced),
    )
    .padding(6)
    .width(Length::Fit.max(420.0))
    .style(container::bordered_box);
    Element::new(ClampedPin {
        content: panel.into(),
        anchor,
        bottom_inset: bottom_inset.max(0.0),
        gap: 12.0,
        offset: Vector::ZERO,
        passive: true,
    })
}

/// Measured, wrapping text in the existing clamped panel layout. Input goes
/// straight through, including clicks directly on the result text.
pub(super) fn measurement_overlay<'a>(
    anchor: iced::Point,
    bottom_inset: f32,
    value: String,
) -> Element<'a, Message> {
    // Inquiry commands return a compact, comma-separated summary for the
    // command line. Show its fields on separate lines in the transient HUD,
    // without altering the command-line history or the measured precision.
    let display_value = if value.contains("\n") {
        value.replace(",  ", "\n").replace(", ", "\n")
    } else {
        value
    };
    let panel = container(text(display_value).size(12))
        .padding([6, 8])
        .width(Length::Fit.max(300.0))
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                text_color: Some(palette.background.weak.text),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        });
    Element::new(ClampedPin {
        content: panel.into(),
        anchor,
        bottom_inset: bottom_inset.max(0.0),
        gap: 18.0,
        offset: Vector::ZERO,
        passive: true,
    })
}

struct ClampedPin<'a> {
    content: Element<'a, Message>,
    anchor: iced::Point,
    bottom_inset: f32,
    gap: f32,
    /// Preferred placement shift from the anchor (applied before fitting).
    offset: Vector,
    /// Readouts prefer above the cursor and never participate in input.
    passive: bool,
}

impl Widget<Message, Theme, iced::Renderer> for ClampedPin<'_> {
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        self.content.as_widget_mut().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        const MARGIN: f32 = 4.0;

        let max = limits.max();
        let safe = Size::new(
            (max.width - MARGIN * 2.0).max(0.0),
            (max.height - self.bottom_inset - MARGIN * 2.0).max(0.0),
        );
        let mut node = self.content.as_widget_mut().layout(
            tree,
            renderer,
            &layout::Limits::new(Size::ZERO, safe),
        );
        let content = node.size();
        let bottom_inset = self.bottom_inset;

        let max_x = (max.width - MARGIN - content.width).max(MARGIN);
        let right = self.anchor.x + self.gap + self.offset.x;
        let left = self.anchor.x - self.gap - content.width;
        let x = if right <= max_x {
            right.max(MARGIN)
        } else if left >= MARGIN {
            left
        } else {
            max_x
        };

        let max_y = (max.height - bottom_inset - MARGIN - content.height).max(MARGIN);
        let below = self.anchor.y + self.gap + self.offset.y;
        let above = self.anchor.y - self.gap - content.height;
        let y = if self.passive && above >= MARGIN && above <= max_y {
            above
        } else if below <= max_y {
            below.max(MARGIN)
        } else if above >= MARGIN {
            above
        } else {
            max_y
        };

        // A cursor over the command-line inset can put even the "above"
        // candidate below the safe area. Keep this passive readout inside it.
        let y = if self.passive { y.min(max_y) } else { y };
        node = node.move_to(iced::Point::new(x, y));
        let size = limits.resolve(Length::Fill, Length::Fill, node.size());
        layout::Node::with_children(size, vec![node])
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content.as_widget_mut().operate(
            tree,
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if self.passive {
            return;
        }
        self.content.as_widget_mut().update(
            tree,
            event,
            layout.children().next().unwrap(),
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.passive {
            return mouse::Interaction::None;
        }
        self.content.as_widget().mouse_interaction(
            tree,
            layout.children().next().unwrap(),
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if let Some(clipped_viewport) = bounds.intersection(viewport) {
            self.content.as_widget().draw(
                tree,
                renderer,
                theme,
                style,
                layout.children().next().unwrap(),
                cursor,
                &clipped_viewport,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            tree,
            layout.children().next().unwrap(),
            renderer,
            viewport,
            translation,
        )
    }
}

// ── Viewport right-click context menu ──────────────────────────────────────

/// Horizontal inset so the pointer rests inside the default row, not on the
/// panel's border, when the menu opens.
const MENU_CURSOR_INSET_X: f32 = 24.0;
/// Width of the icon gutter every row reserves so labels line up whether or
/// not the row carries a glyph (object snaps, command icons).
const MENU_GUTTER_W: f32 = 18.0;
const MENU_ICON_SIZE: f32 = 14.0;
/// Trailing check mark of checked rows; sits in the submenu caret column.
const MENU_CHECK_SIZE: f32 = 10.0;

/// The gutter cell: the row's glyph, faded with a disabled row, or empty
/// space of the same width.
fn context_menu_gutter(icon: Option<MenuIcon>, enabled: bool) -> Element<'static, Message> {
    use crate::ui::icons;
    let chrome = |bytes| {
        if enabled {
            icons::themed::<Message>(bytes, MENU_ICON_SIZE)
        } else {
            icons::themed_disabled::<Message>(bytes, MENU_ICON_SIZE)
        }
    };
    let cell: Element<'static, Message> = match icon {
        Some(MenuIcon::Snap(t)) => chrome(icons::osnap(t)),
        Some(MenuIcon::Mtp) => chrome(icons::mtp_icon()),
        Some(MenuIcon::Chrome(bytes)) => chrome(bytes),
        Some(MenuIcon::Tool(bytes)) if enabled => icons::semantic(bytes, MENU_ICON_SIZE),
        Some(MenuIcon::Tool(bytes)) => icons::semantic_disabled(bytes, MENU_ICON_SIZE),
        None => iced::widget::Space::new()
            .width(MENU_ICON_SIZE)
            .height(MENU_ICON_SIZE)
            .into(),
    };
    container(cell)
        .width(Length::Fixed(MENU_GUTTER_W))
        .align_x(iced::Center)
        .align_y(iced::Center)
        .into()
}

/// Render the right-click context menu (rows from
/// `ui::popup::context_menu::build_context_menu`). The panel is placed so the
/// default row sits under the pointer: right-click then left-click in place
/// completes the step without moving the mouse (commercial solutions anchor their shortcut
/// menu the same way). `highlighted` is the keyboard highlight, if any.
pub(super) fn viewport_context_menu_overlay(
    pos: iced::Point,
    bottom_inset: f32,
    menu: &ContextMenu,
    highlighted: Option<usize>,
) -> Element<'static, Message> {
    let sep = || -> Element<'static, Message> {
        container(iced::widget::Space::new().width(Fill).height(1))
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.weak.color)),
                ..Default::default()
            })
            .width(Fill)
            .height(1)
            .padding([0, 6])
            .into()
    };

    // Index into `menu.selectable()` of the row being rendered, so the
    // keyboard highlight lands on the same row the model counts.
    let mut sel_idx = 0usize;
    let mut items: Vec<Element<'static, Message>> = Vec::new();

    for row in &menu.rows {
        match row {
            MenuRow::Item(item) => {
                let is_hl = highlighted == Some(sel_idx);
                items.push(context_menu_row(item, 0.0, is_hl));
                sel_idx += 1;
            }
            MenuRow::Separator => items.push(sep()),
            MenuRow::Submenu {
                id,
                label,
                icon,
                items: children,
                open,
            } => {
                let is_hl = highlighted == Some(sel_idx);
                let enabled = !children.is_empty();
                let caret = if !enabled {
                    crate::ui::icons::themed_disabled_arrow_right(9.0)
                } else if *open {
                    crate::ui::icons::themed_arrow_down(9.0)
                } else {
                    crate::ui::icons::themed_arrow_right(9.0)
                };
                let content = row![
                    context_menu_gutter(*icon, enabled),
                    text(label.clone())
                        .size(12)
                        .wrapping(iced::widget::text::Wrapping::None),
                    iced::widget::Space::new().width(Fill),
                    caret,
                ]
                .spacing(4)
                .align_y(iced::Center);
                let mut btn = button(content)
                    .padding(iced::Padding {
                        top: 3.0,
                        right: 12.0,
                        bottom: 3.0,
                        left: 6.0,
                    })
                    .width(Fill)
                    .height(Length::Fixed(MENU_ROW_H))
                    .style(move |theme: &Theme, status| {
                        context_menu_row_style(theme, status, is_hl)
                    });
                if enabled {
                    btn = btn.on_press(Message::ContextMenuSubmenuToggle(*id));
                }
                items.push(btn.into());
                sel_idx += 1;
                if *open {
                    for child in children {
                        let is_hl = highlighted == Some(sel_idx);
                        items.push(context_menu_row(child, 14.0, is_hl));
                        sel_idx += 1;
                    }
                }
            }
        }
    }

    let menu_col = column(items).spacing(0).width(Length::Fixed(menu.width));
    let menu_col =
        scrollable(menu_col)
            .height(Length::Shrink)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(8).scroller_width(6),
            ));

    let panel = container(menu_col)
        .style(container::bordered_box)
        .padding([MENU_PAD_TOP as u16, 0])
        .width(Length::Fixed(menu.width));

    // Shift the panel so the default row's centre line is under the pointer.
    let offset = Vector::new(
        -MENU_CURSOR_INSET_X,
        -(menu.default_row_y() + MENU_ROW_H * 0.5),
    );
    position_canvas_overlay_clamped(pos, bottom_inset, offset, panel.into())
}

/// One menu row: label (bold for the default row), the keyword hint
/// right-aligned and dimmed, and a check mark in the trailing column (where
/// submenu headers draw their caret) when checked, so checked labels stay
/// aligned with the others. Disabled rows render without a press handler and
/// with faded text and icons.
fn context_menu_row(item: &MenuItem, indent: f32, highlighted: bool) -> Element<'static, Message> {
    // Rows have a fixed height: a long label must not wrap onto the next
    // row (#2170).
    let mut label_text = text(item.label.clone())
        .size(12)
        .wrapping(iced::widget::text::Wrapping::None);
    if item.default {
        label_text = label_text.font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..iced::Font::DEFAULT
        });
    }
    let enabled = item.enabled;
    let mut content = row![context_menu_gutter(item.icon, enabled), label_text]
        .spacing(4)
        .align_y(iced::Center);
    if let Some(hint) = item.hint.as_ref() {
        content = content.push(iced::widget::Space::new().width(Fill)).push(
            text(hint.clone()).size(11).style(move |theme: &Theme| {
                let palette = theme.palette();
                let base = if highlighted {
                    palette.primary.strong.text
                } else {
                    palette.background.base.text
                };
                iced::widget::text::Style {
                    color: Some(base.scale_alpha(if enabled { 0.6 } else { 0.35 })),
                }
            }),
        );
    }
    if item.checked {
        if item.hint.is_none() {
            content = content.push(iced::widget::Space::new().width(Fill));
        }
        content = content.push(if enabled {
            crate::ui::icons::themed(crate::ui::icons::CHECK, MENU_CHECK_SIZE)
        } else {
            crate::ui::icons::themed_disabled(crate::ui::icons::CHECK, MENU_CHECK_SIZE)
        });
    }
    let mut btn = button(content)
        .padding(iced::Padding {
            top: 3.0,
            right: 12.0,
            bottom: 3.0,
            left: 6.0 + indent,
        })
        .width(Fill)
        .height(Length::Fixed(MENU_ROW_H))
        .style(move |theme: &Theme, status| context_menu_row_style(theme, status, highlighted));
    if enabled {
        btn = btn.on_press(Message::ContextMenuPick(item.action.clone()));
    }
    btn.into()
}

/// Row colours: keyboard highlight uses the accent (as the grip popup does),
/// hover the subtle background tint, disabled rows faded text.
fn context_menu_row_style(
    theme: &Theme,
    status: button::Status,
    highlighted: bool,
) -> button::Style {
    let palette = theme.palette();
    let (background, text_color) = if highlighted {
        (
            Some(Background::Color(palette.primary.strong.color)),
            palette.primary.strong.text,
        )
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => (
                Some(Background::Color(palette.background.weak.color)),
                palette.background.weak.text,
            ),
            button::Status::Disabled => (None, palette.background.base.text.scale_alpha(0.42)),
            button::Status::Active => (None, palette.background.base.text),
        }
    };
    button::Style {
        background,
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

// ── Quad cursor menu ────────────────────────────────────────────────────────

/// Render the quad cursor menu (panels from
/// `ui::popup::quad_menu::quad_panels_for_target`) on its anchor and a
/// full-viewport click catcher: collapsed the single default-command tile,
/// expanded the panel card with its tab strip, the default icon always under
/// the pointer, so a follow-up click in place launches the command with the
/// same cursor discipline as the context menu. The catcher sits *below* the
/// card: an outside press closes the quad (docs/quad-menu.md §3.5).
pub(super) fn viewport_quad_overlay<'a>(
    quad: &QuadUi,
    panels: &[QuadPanel],
    quad_width: usize,
    bottom_inset: f32,
) -> Element<'a, Message> {
    use crate::ui::popup::quad_overlay;

    // Collapsed the quad is one tile; expanded the pinned panel card. An
    // empty primary panel (nothing applicable) has nothing to show.
    let quad_element = if quad.expanded {
        Some(quad_overlay::quad_expanded_card(quad, panels, quad_width))
    } else {
        panels.first().and_then(|panel| {
            panel
                .items
                .first()
                .map(|item| quad_overlay::quad_collapsed_tile(item, &panel.id))
        })
    };

    // The hover quad opens on its own beside the cursor: it catches no clicks
    // and sits up and right of the pointer, so the hovered object can still
    // be clicked, and it closes when the cursor leaves for elsewhere.
    if quad.mode == crate::ui::popup::quad_menu::QuadMode::Hover {
        let Some(content) = quad_element else {
            return iced::widget::Space::new().into();
        };
        let gap = quad_overlay::QUAD_HOVER_GAP;
        let offset = if quad.expanded {
            Vector::new(
                gap - quad_overlay::QUAD_PANEL_PAD,
                -(gap + quad_overlay::QUAD_TILE + quad_overlay::QUAD_PANEL_PAD),
            )
        } else {
            Vector::new(gap, -(gap + quad_overlay::QUAD_TILE))
        };
        return position_canvas_overlay_clamped(quad.anchor, bottom_inset, offset, content);
    }

    let catcher = mouse_area(
        container(iced::widget::Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill),
    )
    .on_press(Message::Quad(QuadMsg::Close))
    .on_right_press(Message::Quad(QuadMsg::Close));

    let Some(content) = quad_element else {
        // No commands at all: the catcher alone lets the next click close it.
        return catcher.into();
    };

    // Keep the default icon's centre under the pointer: the grid pad plus
    // the half tile (collapsed just the half tile).
    let half_tile = quad_overlay::QUAD_TILE * 0.5;
    let offset = if quad.expanded {
        Vector::new(
            -(quad_overlay::QUAD_PANEL_PAD + half_tile),
            -(quad_overlay::QUAD_PANEL_PAD + half_tile),
        )
    } else {
        Vector::new(-half_tile, -half_tile)
    };
    stack![
        catcher,
        position_canvas_overlay_clamped(quad.anchor, bottom_inset, offset, content)
    ]
    .into()
}

/// One-shot snap override menu (Shift+RMB, #337): a cursor-anchored grid of
/// snap ICONS only — the names show as hover tooltips, with the letter that
/// picks each from the keyboard. Picking one applies that snap to just the
/// next point pick; MTP and FROM instead suspend the prompt to compute a point.
pub(super) fn snap_override_overlay(pos: iced::Point) -> Element<'static, Message> {
    const COLS: usize = 4;

    let cell_icon =
        |icon: &'static [u8], label: String, msg: Message| -> Element<'static, Message> {
            let icon = container(crate::ui::icons::themed::<Message>(icon, 16.0))
                .width(26)
                .height(26)
                .align_x(iced::Center)
                .align_y(iced::Center);
            let btn = button(icon)
                .on_press(msg)
                .style(|theme: &Theme, status| button::Style {
                    background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                        .then_some(Background::Color(theme.palette().primary.weak.color)),
                    border: Border::default(),
                    text_color: theme.palette().background.base.text,
                    ..Default::default()
                })
                .padding(2);
            iced::widget::tooltip(
                btn,
                container(text(label).size(11))
                    .style(|theme: &Theme| {
                        let palette = theme.palette();
                        container::Style {
                            background: Some(Background::Color(palette.background.strong.color)),
                            border: Border {
                                color: palette.background.neutral.color,
                                width: 1.0,
                                radius: 2.0.into(),
                            },
                            text_color: Some(palette.background.strong.text),
                            ..Default::default()
                        }
                    })
                    .padding([2, 6]),
                iced::widget::tooltip::Position::Bottom,
            )
            .into()
        };

    // Point modifiers follow the snaps, with the same icon-only cells.
    let mut cells: Vec<Element<'static, Message>> =
        Vec::with_capacity(crate::snap::ALL_SNAP_MODES.len() + 2);
    for &(snap_type, _glyph, label) in crate::snap::ALL_SNAP_MODES {
        // The tooltip teaches the letter that picks the cell from the keyboard.
        let label = match crate::snap::override_menu_key(snap_type) {
            Some(key) => format!("{} ({key})", t!(label)),
            None => t!(label).into_owned(),
        };
        cells.push(cell_icon(
            crate::ui::icons::osnap(snap_type),
            label,
            Message::SnapOverridePick(snap_type),
        ));
    }
    cells.push(cell_icon(
        crate::ui::icons::mtp_icon(),
        t!("Mid Between 2 Points (M2P)").into_owned(),
        Message::SnapOverrideMtp,
    ));
    cells.push(cell_icon(
        crate::ui::icons::from_icon(),
        format!("{} (F)", t!("From")),
        Message::SnapOverrideFrom,
    ));
    let mut grid = column![].spacing(2);
    while !cells.is_empty() {
        let n = cells.len().min(COLS);
        let mut r = row![].spacing(2);
        for c in cells.drain(..n) {
            r = r.push(c);
        }
        grid = grid.push(r);
    }

    let panel = container(grid)
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        })
        .padding(4);

    // Full-screen click-catcher closes on an outside click.
    let catcher = mouse_area(
        container(iced::widget::Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill),
    )
    .on_press(Message::SnapOverrideClose)
    .on_right_press(Message::SnapOverrideClose);

    stack![catcher, position_canvas_overlay(pos, panel.into())].into()
}

// ── Quick Select panel ─────────────────────────────────────────────────────

const QSELECT_ANY_TYPE: &str = "(Any type)";
const QSELECT_ANY_PROP: &str = "(Any property)";

/// Quick Select uses the application's shared movable modal frame. Its form
/// groups scope, filter, and result behavior while reusing the same compact
/// field and button styles as the other workspace dialogs.
pub(super) fn qselect_overlay<'a>(
    state: &'a crate::app::QSelectState,
    types: &[String],
    properties: &[crate::app::QSelectPropertyChoice],
    candidate_count: usize,
    modal_offset: iced::Vector,
    modal_resize: iced::Vector,
) -> Element<'a, Message> {
    let measurement = qselect_content(
        state,
        types,
        properties,
        candidate_count,
        crate::ui::modal::ModalSizing::INTRINSIC,
    );
    let content = qselect_content(
        state,
        types,
        properties,
        candidate_count,
        crate::ui::modal::ModalSizing::FILL,
    );
    // Long imported values must not make the natural dialog span the viewport.
    let content = crate::ui::modal::intrinsic(
        measurement,
        content,
        iced::Size::new(720.0, f32::INFINITY),
        modal_resize,
    );

    crate::ui::modal::modal(
        Space::new().width(Fill).height(Fill),
        t!("Quick Select"),
        content,
        Message::QSelectClose,
        modal_offset,
        crate::ui::modal::ModalOptions::STANDARD,
    )
}

/// Object type label for the selection filter; INSERT reads as a block reference.
fn qselect_type_label(name: &str) -> String {
    if name == "Insert" {
        t!("Block Reference").into_owned()
    } else {
        t!(name).into_owned()
    }
}

fn qselect_content<'a>(
    state: &'a crate::app::QSelectState,
    types: &[String],
    properties: &[crate::app::QSelectPropertyChoice],
    candidate_count: usize,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    use crate::ui::style::form::{dialog_button, dialog_button_styled_opt, form_radio};
    use iced::widget::{checkbox, rule};
    let mut type_options: Vec<String> = vec![QSELECT_ANY_TYPE.to_string()];
    type_options.extend(types.iter().cloned());

    let mut prop_options: Vec<crate::app::QSelectPropertyChoice> =
        vec![crate::app::QSelectPropertyChoice {
            field: String::new(),
            label: t!(QSELECT_ANY_PROP).into_owned(),
            editor: crate::app::QSelectValueEditor::Text,
        }];
    prop_options.extend(properties.iter().cloned());

    let number_property = state
        .property
        .as_ref()
        .is_some_and(|property| matches!(property.editor, crate::app::QSelectValueEditor::Number));
    let mut op_options: Vec<crate::app::QSelectOp> =
        vec![crate::app::QSelectOp::Eq, crate::app::QSelectOp::Neq];
    if number_property {
        op_options.push(crate::app::QSelectOp::Gt);
        op_options.push(crate::app::QSelectOp::Lt);
    }
    op_options.push(crate::app::QSelectOp::Any);

    let type_sel = state
        .type_filter
        .clone()
        .unwrap_or_else(|| QSELECT_ANY_TYPE.to_string());
    let prop_sel = state
        .property
        .clone()
        .unwrap_or(crate::app::QSelectPropertyChoice {
            field: String::new(),
            label: t!(QSELECT_ANY_PROP).into_owned(),
            editor: crate::app::QSelectValueEditor::Text,
        });

    let value_enabled =
        state.property.is_some() && !matches!(state.operator, crate::app::QSelectOp::Any);
    let field_width = if matches!(sizing.width, iced::Length::Fill) {
        Fill
    } else {
        iced::Length::Shrink
    };
    let flex_width = if matches!(sizing.width, iced::Length::Fill) {
        Fill
    } else {
        iced::Length::Shrink
    };

    let label =
        |s: std::borrow::Cow<'static, str>| text(s).size(12).width(iced::Length::Fixed(112.0));
    let section_label = |s: std::borrow::Cow<'static, str>| {
        text(s)
            .size(11)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
            })
    };

    let value_editor: Element<'a, Message> = match state.property.as_ref() {
        Some(property) => match &property.editor {
            crate::app::QSelectValueEditor::Choice(options) => {
                let selected = (!state.value.is_empty()).then(|| state.value.clone());
                let picker =
                    crate::ui::dropdown_list::pick_list(selected, options.clone(), |value| value.to_string())
                        .width(field_width);
                if value_enabled {
                    picker.on_select(Message::QSelectSetValue).into()
                } else {
                    picker.into()
                }
            }
            crate::app::QSelectValueEditor::Text | crate::app::QSelectValueEditor::Number => {
                let mut input = text_input("", &state.value).size(12).width(field_width);
                if value_enabled {
                    input = input.on_input(Message::QSelectSetValue);
                }
                input.into()
            }
        },
        None => text_input("", "").size(12).width(field_width).into(),
    };

    let derived_error = if candidate_count == 0 {
        Some(crate::t!("No objects are available in this scope.").into_owned())
    } else if state.type_filter.is_none() && state.property.is_none() {
        // An empty draft is only an error once Apply is possible; the Add
        // button already requires a type or property.
        None
    } else {
        crate::app::QSelectCondition::validation_error_for(
            state.type_filter.as_deref(),
            state.property.as_ref(),
            state.operator,
            &state.value,
        )
    };
    let error = state.error.clone().or(derived_error.clone());

    let scope_options = vec![
        crate::app::QSelectScope::CurrentSpace,
        crate::app::QSelectScope::CurrentSelection,
    ];
    let scope_picker =
        crate::ui::dropdown_list::pick_list(Some(state.scope), scope_options, |value| value.to_string())
            .on_select(Message::QSelectSetScope)
            .width(field_width);

    let mut append = checkbox(state.append).size(14);
    if matches!(state.scope, crate::app::QSelectScope::CurrentSpace) {
        append = append.on_toggle(Message::QSelectSetAppend);
    }

    let cancel = dialog_button(t!("Cancel"), Message::QSelectClose, false);
    let apply = dialog_button_styled_opt(
        t!("Apply"),
        (candidate_count > 0
            && state.editing_condition.is_none()
            && (state.conditions.is_empty() && error.is_none() || !state.conditions.is_empty()))
        .then_some(Message::QSelectApply),
        button::primary,
    );

    let mut condition_rows = column![].spacing(5);
    if state.conditions.is_empty() {
        condition_rows = condition_rows.push(text(t!("No conditions added.")).size(12));
    }
    for (index, condition) in state.conditions.iter().enumerate() {
        let type_label = condition
            .type_filter
            .as_deref()
            .map(qselect_type_label)
            .unwrap_or_else(|| t!(QSELECT_ANY_TYPE).into_owned());
        let property_label = condition
            .property
            .as_ref()
            .map(|property| property.to_string())
            .unwrap_or_else(|| t!(QSELECT_ANY_PROP).into_owned());
        let summary = format!(
            "{}. {} | {} | {} | {}",
            index + 1,
            type_label,
            property_label,
            condition.operator,
            condition.value
        );
        condition_rows = condition_rows.push(
            row![
                // The measuring copy must not fill the viewport's available width.
                text(summary).size(12).width(flex_width),
                dialog_button(t!("Edit"), Message::QSelectEditCondition(index), false),
                dialog_button(t!("Delete"), Message::QSelectRemoveCondition(index), false),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        );
    }
    let add_condition = dialog_button_styled_opt(
        if state.editing_condition.is_some() {
            t!("Update condition")
        } else {
            t!("Add condition")
        },
        (derived_error.is_none() && (state.type_filter.is_some() || state.property.is_some()))
            .then_some(Message::QSelectAddCondition),
        button::secondary,
    );
    let condition_actions = row![
        add_condition,
        dialog_button_styled_opt(
            t!("Cancel edit"),
            state.editing_condition.map(|_| Message::QSelectCancelEdit),
            button::secondary
        ),
        dialog_button_styled_opt(
            t!("Clear conditions"),
            (!state.conditions.is_empty()).then_some(Message::QSelectClearConditions),
            button::secondary
        ),
    ]
    .spacing(6);

    let panel_body = column![
        section_label(t!("Scope")),
        Space::new().height(5),
        row![label(t!("Apply to:")), scope_picker]
            .align_y(iced::Alignment::Center)
            .spacing(8)
            .width(sizing.width),
        Space::new().height(4),
        text(crate::tf!("{} candidate object(s)", candidate_count))
            .size(11)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
            }),
        Space::new().height(10),
        container(rule::horizontal(1)).width(flex_width),
        Space::new().height(10),
        section_label(t!("Filter")),
        Space::new().height(5),
        row![
            label(t!("Object type:")),
            crate::ui::dropdown_list::pick_list(Some(type_sel), type_options, |value| {
                qselect_type_label(value)
            })
            .on_select(|s: String| {
                if s == QSELECT_ANY_TYPE {
                    Message::QSelectSetType(None)
                } else {
                    Message::QSelectSetType(Some(s))
                }
            })
            .width(field_width),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8)
        .width(sizing.width),
        Space::new().height(6),
        row![
            label(t!("Property:")),
            crate::ui::dropdown_list::pick_list(Some(prop_sel), prop_options, |value| value.to_string(),)
                .on_select(|p: crate::app::QSelectPropertyChoice| {
                    if p.field.is_empty() {
                        Message::QSelectSetProperty(None)
                    } else {
                        Message::QSelectSetProperty(Some(p))
                    }
                })
                .width(field_width),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8)
        .width(sizing.width),
        Space::new().height(6),
        row![
            label(t!("Operator:")),
            crate::ui::dropdown_list::pick_list(Some(state.operator), op_options, |value| value.to_string(),)
                .on_select(Message::QSelectSetOperator)
                .width(field_width),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8)
        .width(sizing.width),
        Space::new().height(6),
        row![label(t!("Value:")), value_editor]
            .align_y(iced::Alignment::Center)
            .spacing(8)
            .width(sizing.width),
        Space::new().height(10),
        container(rule::horizontal(1)).width(flex_width),
        Space::new().height(10),
        condition_actions,
        Space::new().height(8),
        dialog_button_styled_opt(
            t!("Select object"),
            state
                .editing_condition
                .is_none()
                .then_some(Message::QSelectPickObject),
            button::secondary,
        ),
        Space::new().height(8),
        section_label(t!("Conditions (all must match)")),
        Space::new().height(5),
        iced::widget::scrollable(condition_rows)
            .height(120)
            .width(sizing.width),
        Space::new().height(5),
        text(t!(
            "When the list is nonempty, only listed conditions are applied."
        ))
        .size(11),
        Space::new().height(10),
        section_label(t!("Result")),
        Space::new().height(5),
        column![
            form_radio(
                t!("Include matching objects"),
                crate::app::QSelectMode::Include,
                Some(state.mode),
                Message::QSelectSetMode,
            ),
            form_radio(
                t!("Exclude matching objects"),
                crate::app::QSelectMode::Exclude,
                Some(state.mode),
                Message::QSelectSetMode,
            ),
        ]
        .spacing(5),
        Space::new().height(8),
        row![
            append,
            Space::new().width(6),
            text(t!("Append result to current selection")).size(12),
        ]
        .align_y(iced::Alignment::Center),
        Space::new().height(10),
        if let Some(message) = error {
            text(message)
                .size(11)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.palette().danger.base.color),
                })
        } else {
            text("").size(11)
        },
        Space::new().height(8),
        row![
            Space::new().width(flex_width),
            cancel,
            Space::new().width(8),
            apply,
        ]
        .align_y(iced::Alignment::Center),
    ]
    .spacing(0)
    .width(sizing.width)
    .height(sizing.height);

    let panel = container(panel_body)
        .padding(16)
        .width(sizing.width)
        .height(sizing.height)
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }
        });

    panel.into()
}

#[cfg(test)]
mod measurement_tests {
    use super::*;
    #[test]
    fn measurement_overlay_passes_all_clicks_on_text() {
        for make_overlay in [
            measurement_overlay as fn(iced::Point, f32, String) -> Element<'static, Message>,
            grip_tip_overlay,
        ] {
            let underlay = mouse_area(Space::new().width(Fill).height(Fill))
                .on_press(Message::ViewportLeftPress)
                .on_right_press(Message::ViewportRightPress)
                .on_middle_press(Message::ViewportMiddlePress);
            let mut ui = iced_test::simulator(stack![
                underlay,
                make_overlay(
                    iced::Point::new(400.0, 300.0),
                    60.0,
                    "measurement-result".into()
                )
            ]);
            let center = ui
                .find("measurement-result")
                .unwrap()
                .visible_bounds()
                .unwrap()
                .center();
            ui.point_at(center);
            for button in [
                iced::mouse::Button::Left,
                iced::mouse::Button::Right,
                iced::mouse::Button::Middle,
            ] {
                ui.simulate([Event::Mouse(iced::mouse::Event::ButtonPressed(button))]);
                ui.simulate([Event::Mouse(iced::mouse::Event::ButtonReleased(button))]);
            }
            let messages: Vec<_> = ui.into_messages().collect();
            assert!(messages
                .iter()
                .any(|m| matches!(m, Message::ViewportLeftPress)));
            assert!(messages
                .iter()
                .any(|m| matches!(m, Message::ViewportRightPress)));
            assert!(messages
                .iter()
                .any(|m| matches!(m, Message::ViewportMiddlePress)));
        }
    }
    #[test]
    fn measurement_overlay_stays_inside_canvas_above_command_line() {
        for make_overlay in [
            measurement_overlay as fn(iced::Point, f32, String) -> Element<'static, Message>,
            grip_tip_overlay,
        ] {
            for anchor in [iced::Point::new(5.0, 5.0), iced::Point::new(790.0, 595.0)] {
                let mut ui = iced_test::simulator::Simulator::with_size(
                    Default::default(),
                    iced::Size::new(800.0, 600.0),
                    make_overlay(anchor, 90.0, "measurement-result".into()),
                );
                let bounds = ui
                    .find("measurement-result")
                    .unwrap()
                    .visible_bounds()
                    .unwrap();
                assert!(bounds.x >= 4.0 && bounds.y >= 4.0);
                assert!(bounds.x + bounds.width <= 796.0 && bounds.y + bounds.height <= 506.0);
            }
        }
    }
}

#[cfg(test)]
mod quad_view_tests {
    use super::*;
    use crate::ui::popup::quad_menu::{QuadIcon, QuadItem, QuadMode};

    fn quad_item(label: &str, command: &str) -> QuadItem {
        QuadItem {
            label: label.to_string(),
            command: command.to_string(),
            // Distinct two-letter glyph per item so the sim can find each
            // tile by its own text: the label itself only renders inside
            // the bottom tooltip.
            icon: QuadIcon::Glyph(label.chars().take(2).collect::<String>().to_uppercase()),
        }
    }

    fn quad_panels() -> Vec<QuadPanel> {
        vec![QuadPanel {
            id: "quad.primary".to_string(),
            label: String::new(),
            items: vec![quad_item("Move", "MOVE"), quad_item("Copy", "COPY")],
            applies_to: Vec::new(),
        }]
    }

    fn collapsed_quad() -> Element<'static, Message> {
        let quad = QuadUi::open(iced::Point::new(300.0, 225.0), QuadMode::Selection);
        viewport_quad_overlay(&quad, &quad_panels(), 6, 60.0)
    }

    fn expanded_quad() -> Element<'static, Message> {
        let mut quad = QuadUi::open(iced::Point::new(300.0, 225.0), QuadMode::Selection);
        quad.expanded = true;
        viewport_quad_overlay(&quad, &quad_panels(), 6, 60.0)
    }

    /// Press (down + up) at canvas point `at` over the given quad overlay on
    /// a viewport with a passive click surface underneath.
    fn press_at(overlay: Element<'static, Message>, at: iced::Point) -> Vec<Message> {
        let underlay =
            mouse_area(Space::new().width(Fill).height(Fill)).on_press(Message::ViewportLeftPress);
        let mut ui = iced_test::simulator::Simulator::with_size(
            Default::default(),
            iced::Size::new(600.0, 450.0),
            stack![underlay, overlay],
        );
        ui.point_at(at);
        ui.simulate([Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Left,
        ))]);
        ui.simulate([Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        ))]);
        ui.into_messages().collect()
    }

    /// The glyph text of a quad tile, with its visible bounds, for
    /// geometry asserts.
    fn find_tile(
        ui: &mut iced_test::simulator::Simulator<'static, Message>,
        glyph: &str,
    ) -> iced::Rectangle {
        ui.find(glyph)
            .expect("tile glyph is in the tree")
            .visible_bounds()
            .expect("tile glyph is visible")
    }

    #[test]
    fn collapsed_quad_default_tile_picks_in_place_and_expands_on_hover() {
        // The collapsed tile is centred on the anchor: its glyph text sits
        // there, and clicking through (the sim's own click; the tile's
        // bottom tooltip never intercepts) launches the default command in
        // place, expanding the card. Nothing leaks into the viewport.
        let mut ui = iced_test::simulator::Simulator::with_size(
            Default::default(),
            iced::Size::new(600.0, 450.0),
            collapsed_quad(),
        );
        let bounds = find_tile(&mut ui, "MO");
        assert_eq!(
            (bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0),
            (300.0, 225.0),
            "the collapsed tile's glyph is centred on the anchor"
        );
        // A real cursor move at the resting anchor enters the tile first
        // (the sim's cursor starts unavailable; point_at arms it), then the
        // click picks the default command in place.
        ui.point_at(iced::Point::new(300.0, 225.0));
        ui.simulate([Event::Mouse(iced::mouse::Event::CursorMoved {
            position: iced::Point::new(300.0, 225.0),
        })]);
        ui.click("MO");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(messages
            .iter()
            .any(|m| matches!(m, Message::Quad(QuadMsg::Expand(true)))));
        assert!(messages.iter().any(|m| matches!(
            m,
            Message::Quad(QuadMsg::Pick { panel_id, item_index: 0 }) if panel_id == "quad.primary"
        )));
        assert!(!messages
            .iter()
            .any(|m| matches!(m, Message::ViewportLeftPress)));
    }

    #[test]
    fn expanded_quad_grid_tiles_pick_with_panel_id_and_index() {
        // Grid order left-to-right: the second tile sits a tile plus the
        // gap to the right of the anchor-centred first tile. The sim clicks
        // the second tile's own glyph text.
        let mut ui = iced_test::simulator::Simulator::with_size(
            Default::default(),
            iced::Size::new(600.0, 450.0),
            expanded_quad(),
        );
        let second = find_tile(&mut ui, "CO");
        assert_eq!(
            second.x + second.width / 2.0,
            300.0 + crate::ui::popup::quad_overlay::QUAD_TILE
                + crate::ui::popup::quad_overlay::QUAD_TILE_GAP,
            "the second tile's glyph centres a tile plus a gap right of the anchor"
        );
        ui.click("CO");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(messages.iter().any(|m| matches!(
            m,
            Message::Quad(QuadMsg::Pick { panel_id, item_index: 1 }) if panel_id == "quad.primary"
        )));
    }

    #[test]
    fn hover_quad_sits_beside_the_cursor_and_catches_no_click() {
        // The hover quad opens unasked: its tile goes up and right of the
        // cursor, so a click on the hovered object reaches the drawing.
        use crate::ui::popup::quad_overlay::{QUAD_HOVER_GAP, QUAD_TILE};
        let hover = || {
            let quad = QuadUi::open(iced::Point::new(300.0, 225.0), QuadMode::Hover);
            viewport_quad_overlay(&quad, &quad_panels(), 6, 60.0)
        };
        let at_cursor = press_at(hover(), iced::Point::new(300.0, 225.0));
        assert!(!at_cursor.iter().any(|m| matches!(m, Message::Quad(_))), "{at_cursor:?}");
        let middle = QUAD_HOVER_GAP + QUAD_TILE * 0.5;
        let on_tile = press_at(hover(), iced::Point::new(300.0 + middle, 225.0 - middle));
        assert!(on_tile.iter().any(|m| matches!(m, Message::Quad(QuadMsg::Pick { .. }))), "{on_tile:?}");
    }

    #[test]
    fn quad_outside_press_closes() {
        let messages = press_at(collapsed_quad(), iced::Point::new(40.0, 40.0));
        assert!(messages
            .iter()
            .any(|m| matches!(m, Message::Quad(QuadMsg::Close))));
        assert!(!messages
            .iter()
            .any(|m| matches!(m, Message::Quad(QuadMsg::Pick { .. }))));
    }

    #[test]
    fn expanded_quad_card_hugs_the_grid() {
        // The overlay is a full-viewport layer (click catcher + card); the
        // card itself must be sized to its content — the grid's columns
        // (tile + gap) plus the two panel pads — not stretch its panel
        // background across the whole screen.
        let items = quad_panels().first().expect("primary panel").items.len();
        let mut ui = iced_test::simulator::Simulator::with_size(
            Default::default(),
            iced::Size::new(600.0, 450.0),
            expanded_quad(),
        );
        let bounds = ui
            .find(iced::widget::Id::new("quad-expanded-card"))
            .expect("the expanded card is in the tree")
            .visible_bounds()
            .expect("the expanded card is visible");
        use crate::ui::popup::quad_overlay::{QUAD_PANEL_PAD, QUAD_TILE, QUAD_TILE_GAP};
        // One row of `min(cols, items)` tiles: widths (tile + gap) minus the
        // trailing gap, then the two pads around the grid.
        let expected = items.min(6) as f32 * (QUAD_TILE + QUAD_TILE_GAP) - QUAD_TILE_GAP
            + 2.0 * QUAD_PANEL_PAD;
        assert_eq!(
            bounds.width, expected,
            "the card hugs the icon grid plus padding (quad_width=6)"
        );
        assert!(
            bounds.width < 300.0,
            "the card must not stretch across the viewport"
        );
    }
}
