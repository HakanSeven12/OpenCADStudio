//! Linetype manager: the shared style-manager frame with a pattern editor, a
//! preview drawn from the real pattern, the linetype scales and the library.

use crate::app::Message;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::labeled_field;
use crate::ui::style::style_manager::{
    self, tb_button, tb_button_maybe, EditorShell, EditorTab, Scaffold, StyleRow,
};
use iced::widget::{button, canvas, checkbox, column, row, text, text_input, Space};
use iced::{mouse, Element, Length, Point, Rectangle, Theme};
use std::borrow::Cow;

pub const PATTERN_TAB: u8 = 0;
pub const SCALE_TAB: u8 = 1;
pub const LIBRARY_TAB: u8 = 2;
const MAX_ROWS: usize = 12;
/// Repeats of the pattern across the preview.
const PREVIEW_REPEATS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Dash,
    Gap,
    Dot,
}

const ELEMENT_KINDS: [ElementKind; 3] = [ElementKind::Dash, ElementKind::Gap, ElementKind::Dot];

impl ElementKind {
    fn label(self) -> String {
        match self {
            Self::Dash => t!("Dash"),
            Self::Gap => t!("Gap"),
            Self::Dot => t!("Dot"),
        }
        .into_owned()
    }
}

#[derive(Debug, Clone)]
pub enum LinetypeMsg {
    Open,
    Select(String),
    Tab(u8),
    InUseFilter(bool),
    New,
    Copy,
    Delete,
    Import,
    SetCurrent,
    Apply,
    Description(String),
    ElementKind(usize, ElementKind),
    ElementLength(usize, String),
    ElementAdd,
    ElementDelete(usize),
    ElementMove(usize, bool),
    Scale(&'static str, String),
    ScaleToggle(&'static str, bool),
    ListScrolled(ListView),
    Export { all: bool },
    ExportPicked { all: bool, path: Option<String> },
    SaveToLibrary,
    ReloadLibrary,
}

/// Where the list is scrolled, as the list last reported it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListView {
    pub offset: f32,
    pub height: f32,
    pub content: f32,
}

/// Rows sit one pixel apart.
const ROW_SPACING: f32 = 1.0;

impl ListView {
    /// The offset that brings row `index` of `count` fully into view, moving
    /// as little as possible. `None` when it is already in view.
    pub fn offset_for_row(self, index: usize, count: usize) -> Option<f32> {
        if count == 0 {
            return None;
        }
        let pitch = (self.content + ROW_SPACING) / count as f32;
        let top = pitch * index as f32;
        let bottom = top + pitch - ROW_SPACING;
        if top < self.offset {
            Some(top)
        } else if bottom > self.offset + self.height {
            Some(bottom - self.height)
        } else {
            None
        }
    }
}

fn msg(message: LinetypeMsg) -> Message {
    Message::Linetype(message)
}

/// One editable element of the pattern. A gap's length is typed as a positive
/// number; a dot has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternRow {
    pub kind: ElementKind,
    pub length: String,
}

/// What the editor holds until it is written to the linetype.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinetypeDraft {
    pub description: String,
    pub rows: Vec<PatternRow>,
    pub ltscale: String,
    pub celtscale: String,
    pub msltscale: bool,
    pub psltscale: bool,
}

impl LinetypeDraft {
    pub fn set_pattern(&mut self, lengths: impl IntoIterator<Item = f64>) {
        self.rows = lengths
            .into_iter()
            .map(|length| PatternRow {
                kind: if length > 0.0 {
                    ElementKind::Dash
                } else if length < 0.0 {
                    ElementKind::Gap
                } else {
                    ElementKind::Dot
                },
                length: if length == 0.0 {
                    String::new()
                } else {
                    length.abs().to_string()
                },
            })
            .collect();
    }

    /// The rows as `.lin` lengths, or `None` when one is not a positive number.
    pub fn pattern(&self) -> Option<Vec<f64>> {
        self.rows
            .iter()
            .map(|row| {
                if row.kind == ElementKind::Dot {
                    return Some(0.0);
                }
                let length = row.length.trim().parse::<f64>().ok()?;
                if !length.is_finite() || length <= 0.0 {
                    return None;
                }
                Some(if row.kind == ElementKind::Gap {
                    -length
                } else {
                    length
                })
            })
            .collect()
    }

    pub fn scales(&self) -> Option<(f64, f64)> {
        let parse = |value: &str| {
            value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v > 0.0)
        };
        Some((parse(&self.ltscale)?, parse(&self.celtscale)?))
    }
}

/// One element for the preview. `label` is the text it carries; `shape`
/// marks one that carries a shape.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewElement {
    pub length: f64,
    pub label: Option<String>,
    pub shape: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PreviewMark {
    Dash { from: f32, to: f32 },
    Dot { at: f32 },
    Label { at: f32, text: String },
    Shape { at: f32 },
}

/// Where the pattern's marks fall across `width`, a few repeats wide so its
/// rhythm shows at any scale. No pattern is one solid line.
pub fn preview_marks(elements: &[PreviewElement], width: f32) -> Vec<PreviewMark> {
    let period: f64 = elements.iter().map(|element| element.length.abs()).sum();
    if elements.is_empty() || period <= 0.0 || !period.is_finite() {
        return vec![PreviewMark::Dash {
            from: 0.0,
            to: width,
        }];
    }
    let scale = f64::from(width) / (period * PREVIEW_REPEATS as f64);
    let mut marks = Vec::new();
    let mut pen = 0.0_f64;
    for _ in 0..PREVIEW_REPEATS {
        for element in elements {
            let at = (pen * scale) as f32;
            if element.length > 0.0 {
                marks.push(PreviewMark::Dash {
                    from: at,
                    to: ((pen + element.length) * scale) as f32,
                });
            } else if element.length == 0.0 {
                marks.push(PreviewMark::Dot { at });
            }
            if let Some(label) = &element.label {
                marks.push(PreviewMark::Label {
                    at,
                    text: label.clone(),
                });
            }
            if element.shape {
                marks.push(PreviewMark::Shape { at });
            }
            pen += element.length.abs();
        }
    }
    marks
}

struct PreviewCanvas {
    elements: Vec<PreviewElement>,
}

impl canvas::Program<Message> for PreviewCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        const MARGIN: f32 = 16.0;
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let ink = theme.palette().background.base.text;
        let y = bounds.height * 0.5;
        let stroke = canvas::Stroke::default().with_color(ink).with_width(1.6);
        for mark in preview_marks(&self.elements, bounds.width - MARGIN * 2.0) {
            match mark {
                PreviewMark::Dash { from, to } => frame.stroke(
                    &canvas::Path::line(Point::new(from + MARGIN, y), Point::new(to + MARGIN, y)),
                    stroke,
                ),
                PreviewMark::Dot { at } => {
                    frame.fill(&canvas::Path::circle(Point::new(at + MARGIN, y), 1.6), ink)
                }
                // A drawn diamond: a glyph may be missing from the font.
                PreviewMark::Shape { at } => {
                    let (x, r) = (at + MARGIN, 4.0);
                    let diamond = canvas::Path::new(|path| {
                        path.move_to(Point::new(x, y - r));
                        path.line_to(Point::new(x + r, y));
                        path.line_to(Point::new(x, y + r));
                        path.line_to(Point::new(x - r, y));
                        path.close();
                    });
                    frame.fill(&diamond, ink);
                }
                PreviewMark::Label { at, text } => frame.fill_text(canvas::Text {
                    content: text,
                    position: crate::ui::style::text::snap_point(Point::new(at + MARGIN, y)),
                    color: ink,
                    size: iced::Pixels(12.0),
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                }),
            }
        }
        vec![frame.into_geometry()]
    }
}

pub struct LinetypeView<'a> {
    pub styles: Vec<String>,
    pub rows: Vec<StyleRow>,
    pub selected: &'a str,
    pub current: String,
    pub tab: u8,
    pub in_use_only: bool,
    pub draft: &'a LinetypeDraft,
    /// Built-ins, referenced linetypes and those with text or shapes are
    /// shown but their pattern is not edited here.
    pub editable: bool,
    pub complex: bool,
    /// ByLayer, ByBlock, Continuous or a referenced linetype.
    pub locked: bool,
    pub in_use: bool,
    pub preview: Vec<PreviewElement>,
    pub apply_blocked: Option<Cow<'static, str>>,
    pub rename_active: Option<&'a str>,
    pub rename_buf: &'a str,
    pub library_status: &'a str,
}

fn pattern_tab<'a>(v: &LinetypeView<'a>) -> Element<'a, Message> {
    let editable = v.editable;
    let draft = v.draft;
    let mut description = text_input("", &draft.description)
        .size(11)
        .width(Length::Fill);
    if editable {
        description = description.on_input(|value| msg(LinetypeMsg::Description(value)));
    }
    let mut items = column![row![
        text(t!("Description"))
            .size(11)
            .style(muted_style)
            .width(90),
        description,
    ]
    .spacing(8)
    .align_y(iced::Center),]
    .spacing(6);
    if v.complex {
        items = items.push(
            text(t!(
                "This linetype has text or shape elements. Its pattern cannot be edited here yet."
            ))
            .size(11)
            .style(muted_style),
        );
    }
    if !draft.rows.is_empty() || editable {
        let add = button(text(t!("Add")).size(10))
            .on_press_maybe(
                (editable && draft.rows.len() < MAX_ROWS).then_some(msg(LinetypeMsg::ElementAdd)),
            )
            .padding([4, 12]);
        items = items.push(
            row![
                text(t!("Type")).size(10).style(muted_style).width(120),
                text(t!("Length")).size(10).style(muted_style).width(120),
                Space::new().width(Length::Fill),
                add,
            ]
            .spacing(6)
            .align_y(iced::Center),
        );
    }
    let last = draft.rows.len().saturating_sub(1);
    for (index, element) in draft.rows.iter().enumerate() {
        let kind: Element<'a, Message> = if editable {
            crate::ui::dropdown_list::pick_list(
                Some(element.kind),
                ELEMENT_KINDS.to_vec(),
                |kind: &ElementKind| kind.label(),
            )
            .on_select(move |kind| msg(LinetypeMsg::ElementKind(index, kind)))
            .text_size(11)
            .width(120)
            .into()
        } else {
            text(element.kind.label()).size(11).width(120).into()
        };
        let mut length = text_input("", &element.length).size(11).width(120);
        if editable && element.kind != ElementKind::Dot {
            length = length.on_input(move |value| msg(LinetypeMsg::ElementLength(index, value)));
        }
        let mut line = row![kind, length, Space::new().width(Length::Fill)]
            .spacing(6)
            .align_y(iced::Center);
        if editable {
            line = line
                .push(
                    button(text(t!("Up")).size(10))
                        .on_press_maybe(
                            (index > 0).then_some(msg(LinetypeMsg::ElementMove(index, true))),
                        )
                        .padding([4, 10]),
                )
                .push(
                    button(text(t!("Down")).size(10))
                        .on_press_maybe(
                            (index < last).then_some(msg(LinetypeMsg::ElementMove(index, false))),
                        )
                        .padding([4, 10]),
                )
                .push(
                    button(text(t!("Delete")).size(10))
                        .on_press(msg(LinetypeMsg::ElementDelete(index)))
                        .padding([4, 10]),
                );
        }
        items = items.push(line);
    }
    items.into()
}

fn scale_tab<'a>(v: &LinetypeView<'a>) -> Element<'a, Message> {
    column![
        labeled_field(
            t!("Global scale (LTSCALE)"),
            &v.draft.ltscale,
            |value| msg(LinetypeMsg::Scale("ltscale", value)),
            120,
        ),
        labeled_field(
            t!("Scale of new objects (CELTSCALE)"),
            &v.draft.celtscale,
            |value| msg(LinetypeMsg::Scale("celtscale", value)),
            120,
        ),
        checkbox(v.draft.msltscale)
            .label(t!("Scale model space by the annotation scale (MSLTSCALE)"))
            .on_toggle(|on| msg(LinetypeMsg::ScaleToggle("msltscale", on)))
            .size(14)
            .text_size(11),
        checkbox(v.draft.psltscale)
            .label(t!(
                "Scale layout viewports by the viewport scale (PSLTSCALE)"
            ))
            .on_toggle(|on| msg(LinetypeMsg::ScaleToggle("psltscale", on)))
            .size(14)
            .text_size(11),
    ]
    .spacing(10)
    .into()
}

fn library_tab<'a>(v: &LinetypeView<'a>) -> Element<'a, Message> {
    let can_save = !v.locked && v.preview.iter().any(|element| element.length != 0.0);
    column![
        text(t!(
            "Write linetypes to a file, or keep one for every drawing."
        ))
        .size(11)
        .style(muted_style),
        row![
            tb_button(
                t!("Export Selected..."),
                msg(LinetypeMsg::Export { all: false }),
                false
            ),
            tb_button(
                t!("Export All..."),
                msg(LinetypeMsg::Export { all: true }),
                false
            ),
        ]
        .spacing(6),
        row![
            tb_button_maybe(
                t!("Save to My Library"),
                can_save.then_some(msg(LinetypeMsg::SaveToLibrary)),
                false
            ),
            tb_button(t!("Reload Library"), msg(LinetypeMsg::ReloadLibrary), false),
        ]
        .spacing(6),
        text(v.library_status.to_string())
            .size(11)
            .style(muted_style),
    ]
    .spacing(10)
    .into()
}

pub fn view_window<'a>(
    v: LinetypeView<'a>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let content = match v.tab {
        SCALE_TAB => scale_tab(&v),
        LIBRARY_TAB if !cfg!(target_arch = "wasm32") => library_tab(&v),
        _ => pattern_tab(&v),
    };
    let status = if v.selected.eq_ignore_ascii_case(&v.current) {
        t!("Current style")
    } else if v.in_use {
        t!("Used in drawing")
    } else {
        t!("Available style")
    };
    let tab = |label: Cow<'static, str>, id: u8| EditorTab {
        label,
        active: v.tab == id,
        on_press: msg(LinetypeMsg::Tab(id)),
    };
    let mut tabs = vec![tab(t!("Pattern"), PATTERN_TAB), tab(t!("Scale"), SCALE_TAB)];
    if !cfg!(target_arch = "wasm32") {
        tabs.push(tab(t!("Library"), LIBRARY_TAB));
    }
    let preview = canvas(PreviewCanvas {
        elements: v.preview.clone(),
    })
    .width(Length::Fill)
    .height(90)
    .into();
    let editor = style_manager::editor_shell(EditorShell {
        sizing,
        selected: v.selected.to_string(),
        status,
        preview,
        comparison: None,
        tabs,
        content,
    });
    style_manager::view(Scaffold {
        sizing,
        kind: crate::app::StyleKind::Linetype,
        styles: &v.styles,
        selected: v.selected,
        current: Some(v.current.as_str()),
        rename_active: v.rename_active,
        rename_buf: v.rename_buf,
        on_new: msg(LinetypeMsg::New),
        on_copy: msg(LinetypeMsg::Copy),
        on_delete: msg(LinetypeMsg::Delete),
        on_import: Some(msg(LinetypeMsg::Import)),
        on_select: |name| msg(LinetypeMsg::Select(name)),
        on_set_current: msg(LinetypeMsg::SetCurrent),
        on_apply: msg(LinetypeMsg::Apply),
        read_only: v.locked,
        locked_keeps_actions: true,
        on_list_scroll: Some(|viewport| {
            msg(LinetypeMsg::ListScrolled(ListView {
                offset: viewport.absolute_offset().y,
                height: viewport.bounds().height,
                content: viewport.content_bounds().height,
            }))
        }),
        in_use_filter: Some((v.in_use_only, |on| msg(LinetypeMsg::InUseFilter(on)))),
        apply_blocked: v.apply_blocked,
        rows: Some(&v.rows),
        compact: false,
        editor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lengths: &[f64]) -> Vec<PreviewElement> {
        lengths
            .iter()
            .map(|length| PreviewElement {
                length: *length,
                label: None,
                shape: false,
            })
            .collect()
    }

    #[test]
    fn the_preview_places_an_asymmetric_pattern_exactly() {
        // Period 3, drawn three times across 90: 10 pixels per unit.
        let marks = preview_marks(&plain(&[1.5, -0.25, 0.0, -1.25]), 90.0);
        assert_eq!(marks.len(), 6, "{marks:?}");
        assert_eq!(
            marks[0],
            PreviewMark::Dash {
                from: 0.0,
                to: 15.0
            }
        );
        assert_eq!(marks[1], PreviewMark::Dot { at: 17.5 });
        assert_eq!(
            marks[2],
            PreviewMark::Dash {
                from: 30.0,
                to: 45.0
            }
        );
        assert_eq!(marks[3], PreviewMark::Dot { at: 47.5 });
        assert_eq!(marks[5], PreviewMark::Dot { at: 77.5 });
    }

    #[test]
    fn a_label_rides_at_its_element() {
        let mut elements = plain(&[2.0, -1.0]);
        elements[1].label = Some("GAS".into());
        let marks = preview_marks(&elements, 90.0);
        assert_eq!(
            marks[1],
            PreviewMark::Label {
                at: 20.0,
                text: "GAS".into()
            }
        );
    }

    #[test]
    fn no_pattern_is_one_solid_line() {
        for elements in [plain(&[]), plain(&[0.0, 0.0])] {
            assert_eq!(
                preview_marks(&elements, 200.0),
                vec![PreviewMark::Dash {
                    from: 0.0,
                    to: 200.0
                }]
            );
        }
    }

    /// 20 rows of 22 with 1 between: 459 tall, showing 100.
    fn list(offset: f32) -> ListView {
        ListView {
            offset,
            height: 100.0,
            content: 459.0,
        }
    }

    #[test]
    fn a_row_in_view_does_not_scroll_the_list() {
        for index in 0..4 {
            assert_eq!(list(0.0).offset_for_row(index, 20), None, "{index}");
        }
        assert_eq!(list(92.0).offset_for_row(5, 20), None);
    }

    #[test]
    fn the_list_scrolls_only_as_far_as_the_row_needs() {
        // Row 4 ends at 114: 14 past the bottom of the view.
        assert_eq!(list(0.0).offset_for_row(4, 20), Some(14.0));
        // Row 3 starts at 69, above a view from 92.
        assert_eq!(list(92.0).offset_for_row(3, 20), Some(69.0));
        assert_eq!(
            list(359.0).offset_for_row(19, 20),
            None,
            "already at the end"
        );
        assert_eq!(list(0.0).offset_for_row(0, 0), None);
    }

    #[test]
    fn a_draft_turns_rows_into_lengths() {
        let mut draft = LinetypeDraft::default();
        draft.set_pattern([1.5, -0.25, 0.0, -0.75]);
        assert_eq!(draft.rows[1].kind, ElementKind::Gap);
        assert_eq!(draft.rows[1].length, "0.25");
        assert_eq!(draft.rows[2].length, "");
        assert_eq!(draft.pattern(), Some(vec![1.5, -0.25, 0.0, -0.75]));
        for bad in ["", "abc", "0", "-1", "NaN", "inf"] {
            draft.rows[0].length = bad.to_owned();
            assert_eq!(draft.pattern(), None, "{bad:?}");
        }
    }
}
