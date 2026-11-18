//! A one-line formula box whose `%<…>%` operands show as labelled chips
//! (#2532). The caret steps over an operand, Backspace and Delete remove it
//! whole, and the value it edits stays the formula code.

use std::ops::Range;

use iced::advanced::layout::{self, Layout};
use iced::advanced::text::{self, Renderer as _};
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{clipboard, mouse, renderer, Renderer as _, Shell};
use iced::keyboard::{self, key::Named, Key};
use iced::{
    alignment, Border, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Renderer,
    Size, Theme,
};

use crate::ui::style::text_metrics;

const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 8.0;
const CHIP_PAD: f32 = 4.0;
/// Labels longer than this many characters end in an ellipsis.
const LABEL_CHARS: usize = 28;

/// Byte ranges of the top-level `%<…>%` operands of `formula`; an operand
/// nested in another belongs to its span. An unclosed `%<` stays text.
pub fn operand_spans(formula: &str) -> Vec<Range<usize>> {
    let bytes = formula.as_bytes();
    let mut spans = Vec::new();
    let (mut depth, mut start, mut i) = (0usize, 0usize, 0usize);
    while i + 1 < bytes.len() {
        match (bytes[i], bytes[i + 1]) {
            (b'%', b'<') => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
                i += 2;
            }
            (b'>', b'%') if depth > 0 => {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    spans.push(start..i);
                }
            }
            _ => i += 1,
        }
    }
    spans
}

/// The formula box: `value` is the formula code, `labels` the names its
/// operands show, in order.
pub fn formula_input<'a, Message>(
    value: &'a str,
    labels: &'a [String],
    placeholder: &'a str,
    on_change: impl Fn(String) -> Message + 'a,
) -> FormulaInput<'a, Message> {
    FormulaInput {
        value,
        labels,
        placeholder,
        size: 12.0,
        on_change: Box::new(on_change),
    }
}

pub struct FormulaInput<'a, Message> {
    value: &'a str,
    labels: &'a [String],
    placeholder: &'a str,
    size: f32,
    on_change: Box<dyn Fn(String) -> Message + 'a>,
}

#[derive(Default)]
struct State {
    focused: bool,
    /// Byte offset of the caret; never inside an operand.
    caret: usize,
    /// The other end of the selection; no selection when it is the caret.
    anchor: usize,
    dragging: bool,
    pasting: bool,
    modifiers: keyboard::Modifiers,
}

impl State {
    fn selection(&self) -> Option<Range<usize>> {
        (self.anchor != self.caret)
            .then(|| self.anchor.min(self.caret)..self.anchor.max(self.caret))
    }
}

enum Piece {
    Text(Range<usize>),
    Operand(Range<usize>, usize),
}

impl<'a, Message> FormulaInput<'a, Message> {
    fn pieces(&self) -> Vec<Piece> {
        let mut pieces = Vec::new();
        let mut at = 0;
        for (i, span) in operand_spans(self.value).into_iter().enumerate() {
            if span.start > at {
                pieces.push(Piece::Text(at..span.start));
            }
            at = span.end;
            pieces.push(Piece::Operand(span, i));
        }
        if at < self.value.len() {
            pieces.push(Piece::Text(at..self.value.len()));
        }
        pieces
    }

    fn label(&self, index: usize, span: &Range<usize>) -> String {
        let label = self
            .labels
            .get(index)
            .map_or(&self.value[span.clone()], String::as_str);
        if label.chars().count() > LABEL_CHARS {
            let cut: String = label.chars().take(LABEL_CHARS - 1).collect();
            format!("{cut}…")
        } else {
            label.to_string()
        }
    }

    /// Width of `run` on one line; the trailing bar keeps trailing spaces.
    fn text_width(&self, run: &str) -> f32 {
        if run.is_empty() {
            return 0.0;
        }
        text_metrics::width(&format!("{run}|"), self.size, false)
            - text_metrics::width("|", self.size, false)
    }

    /// Every caret position (byte offset) with its x from the text start.
    fn stops(&self) -> Vec<(usize, f32)> {
        let mut stops = Vec::new();
        let mut x = 0.0;
        for piece in self.pieces() {
            match piece {
                Piece::Text(range) => {
                    let run = &self.value[range.clone()];
                    for (k, _) in run.char_indices() {
                        stops.push((range.start + k, x + self.text_width(&run[..k])));
                    }
                    x += self.text_width(run);
                }
                Piece::Operand(span, i) => {
                    stops.push((span.start, x));
                    x += self.text_width(&self.label(i, &span)) + 2.0 * CHIP_PAD;
                }
            }
        }
        stops.push((self.value.len(), x));
        stops
    }

    fn line_height(&self) -> f32 {
        (self.size * 1.3).ceil()
    }

    /// How far the text is scrolled left so the caret stays in view.
    fn scroll(stops: &[(usize, f32)], caret: usize, inner_width: f32) -> f32 {
        (x_of(stops, caret) - inner_width + 2.0).max(0.0)
    }

    fn edit(
        &self,
        state: &mut State,
        range: Range<usize>,
        insert: &str,
        shell: &mut Shell<'_, Message>,
    ) {
        if range.is_empty() && insert.is_empty() {
            return;
        }
        let mut value = String::with_capacity(self.value.len() + insert.len());
        value.push_str(&self.value[..range.start]);
        value.push_str(insert);
        value.push_str(&self.value[range.end..]);
        state.caret = range.start + insert.len();
        state.anchor = state.caret;
        shell.publish((self.on_change)(value));
    }
}

fn x_of(stops: &[(usize, f32)], at: usize) -> f32 {
    stops
        .iter()
        .rev()
        .find(|(byte, _)| *byte <= at)
        .map_or(0.0, |(_, x)| *x)
}

/// The caret position nearest to `x`.
fn nearest(stops: &[(usize, f32)], x: f32) -> usize {
    stops
        .iter()
        .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
        .map_or(0, |(byte, _)| *byte)
}

/// A position that is a caret stop: `at`, or the stop before it.
fn snap(stops: &[(usize, f32)], at: usize) -> usize {
    stops
        .iter()
        .rev()
        .find(|(byte, _)| *byte <= at)
        .map_or(0, |(byte, _)| *byte)
}

impl<Message> Widget<Message, Theme, Renderer> for FormulaInput<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let height = self.line_height() + 2.0 * PAD_Y;
        layout::atomic(limits, Length::Fill, Length::Fixed(height))
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let stops = self.stops();
        // The value may have changed under the caret (an operand added).
        state.caret = snap(&stops, state.caret.min(self.value.len()));
        state.anchor = snap(&stops, state.anchor.min(self.value.len()));
        let inner_width = bounds.width - 2.0 * PAD_X;
        let hit = |position: Point, caret: usize| {
            let x = position.x - bounds.x - PAD_X + Self::scroll(&stops, caret, inner_width);
            nearest(&stops, x)
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_over(bounds) {
                    let at = hit(position, state.caret);
                    if !(state.modifiers.shift() && state.focused) {
                        state.anchor = at;
                    }
                    state.caret = at;
                    state.focused = true;
                    state.dragging = true;
                    shell.request_redraw();
                    shell.capture_event();
                } else if state.focused {
                    state.focused = false;
                    state.anchor = state.caret;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                if let Some(position) = cursor.position() {
                    state.caret = hit(position, state.caret);
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                physical_key,
                modifiers,
                ..
            }) if state.focused => {
                let command = modifiers.command() && !modifiers.alt();
                let select = modifiers.shift();
                let previous = stops
                    .iter()
                    .rev()
                    .find(|(byte, _)| *byte < state.caret)
                    .map_or(0, |(byte, _)| *byte);
                let next = stops
                    .iter()
                    .find(|(byte, _)| *byte > state.caret)
                    .map_or(self.value.len(), |(byte, _)| *byte);
                let move_to =|state: &mut State, at: usize| {
                    state.caret = at;
                    if !select {
                        state.anchor = at;
                    }
                };
                if command {
                    match key.to_latin(*physical_key) {
                        Some('a') => {
                            state.anchor = 0;
                            state.caret = self.value.len();
                        }
                        Some(c @ ('c' | 'x')) => {
                            if let Some(range) = state.selection() {
                                shell.write_clipboard(clipboard::Content::Text(
                                    self.value[range.clone()].to_string(),
                                ));
                                if c == 'x' {
                                    self.edit(state, range, "", shell);
                                }
                            }
                        }
                        Some('v') => {
                            state.pasting = true;
                            shell.read_clipboard(clipboard::Kind::Text);
                        }
                        _ => return,
                    }
                    shell.request_redraw();
                    shell.capture_event();
                    return;
                }
                match key {
                    Key::Named(Named::ArrowLeft) => match state.selection() {
                        Some(range) if !select => move_to(state, range.start),
                        _ => move_to(state, previous),
                    },
                    Key::Named(Named::ArrowRight) => match state.selection() {
                        Some(range) if !select => move_to(state, range.end),
                        _ => move_to(state, next),
                    },
                    Key::Named(Named::Home) => move_to(state, 0),
                    Key::Named(Named::End) => move_to(state, self.value.len()),
                    Key::Named(Named::Backspace) => {
                        let range = state.selection().unwrap_or(previous..state.caret);
                        self.edit(state, range, "", shell);
                    }
                    Key::Named(Named::Delete) => {
                        let range = state.selection().unwrap_or(state.caret..next);
                        self.edit(state, range, "", shell);
                    }
                    Key::Named(Named::Escape | Named::Enter | Named::Tab) => return,
                    _ => {
                        let Some(typed) = text
                            .as_deref()
                            .filter(|t| !t.chars().any(char::is_control))
                        else {
                            return;
                        };
                        let range = state.selection().unwrap_or(state.caret..state.caret);
                        self.edit(state, range, typed, shell);
                    }
                }
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Clipboard(clipboard::Event::Read(Ok(content))) if state.pasting => {
                state.pasting = false;
                if let clipboard::Content::Text(pasted) = content.as_ref() {
                    let line = pasted.replace(['\r', '\n'], " ");
                    let range = state.selection().unwrap_or(state.caret..state.caret);
                    self.edit(state, range, &line, shell);
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let palette = theme.palette();
        let bounds = layout.bounds();
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    color: if state.focused {
                        palette.primary.base.color
                    } else {
                        palette.background.neutral.color
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..renderer::Quad::default()
            },
            palette.background.base.color,
        );
        let inner = Rectangle {
            x: bounds.x + PAD_X,
            y: bounds.y + 1.0,
            width: bounds.width - 2.0 * PAD_X,
            height: bounds.height - 2.0,
        };
        let line = self.line_height();
        let centre_y = bounds.center_y();
        let stops = self.stops();
        let caret = snap(&stops, state.caret.min(self.value.len()));
        let left = inner.x - Self::scroll(&stops, caret, inner.width);
        let size = self.size;
        let draw_text = |renderer: &mut Renderer, content: &str, x: f32, color: Color| {
            renderer.fill_text(
                text::Text {
                    content: content.to_string(),
                    bounds: Size::new(f32::INFINITY, line),
                    size: Pixels(size),
                    line_height: text::LineHeight::default(),
                    font: Font::DEFAULT,
                    align_x: text::Alignment::Left,
                    align_y: alignment::Vertical::Center,
                    shaping: text::Shaping::default(),
                    wrapping: text::Wrapping::None,
                    ellipsis: text::Ellipsis::None,
                    hint_factor: None,
                },
                crate::ui::style::text::snap_point(Point::new(x, centre_y)),
                color,
                *viewport,
            );
        };
        renderer.with_layer(inner, |renderer| {
            if self.value.is_empty() && !state.focused {
                draw_text(
                    renderer,
                    self.placeholder,
                    inner.x,
                    crate::ui::style::readable::hint_text(theme),
                );
            }
            if let Some(range) = state.selection().filter(|_| state.focused) {
                let (from, to) = (x_of(&stops, range.start), x_of(&stops, range.end));
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new(left + from, centre_y - line / 2.0),
                            Size::new(to - from, line),
                        ),
                        ..renderer::Quad::default()
                    },
                    palette.primary.base.color.scale_alpha(0.5),
                );
            }
            for piece in self.pieces() {
                match piece {
                    Piece::Text(range) => draw_text(
                        renderer,
                        &self.value[range.clone()],
                        left + x_of(&stops, range.start),
                        palette.background.base.text,
                    ),
                    Piece::Operand(span, i) => {
                        let label = self.label(i, &span);
                        let x = left + x_of(&stops, span.start);
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    Point::new(x + 1.0, centre_y - line / 2.0 - 1.0),
                                    Size::new(
                                        self.text_width(&label) + 2.0 * CHIP_PAD - 2.0,
                                        line + 2.0,
                                    ),
                                ),
                                border: Border {
                                    radius: 3.0.into(),
                                    ..Border::default()
                                },
                                ..renderer::Quad::default()
                            },
                            palette.background.weak.color,
                        );
                        draw_text(renderer, &label, x + CHIP_PAD, palette.background.weak.text);
                    }
                }
            }
            if state.focused {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new((left + x_of(&stops, caret)).round(), centre_y - line / 2.0),
                            Size::new(1.0, line),
                        ),
                        ..renderer::Quad::default()
                    },
                    palette.background.base.text,
                );
            }
        });
    }
}

impl<'a, Message: 'a> From<FormulaInput<'a, Message>> for Element<'a, Message> {
    fn from(input: FormulaInput<'a, Message>) -> Self {
        Element::new(input)
    }
}

