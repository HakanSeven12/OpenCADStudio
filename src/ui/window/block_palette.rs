//! Docked "Insert Block" panel: a searchable grid of block thumbnails that
//! inserts a clicked block, mirrors the existing `InsertBlockCommand` flow.

use crate::app::Message;
use crate::modules::IconKind;
use crate::scene::model::wire_model::WireModel;
use iced::widget::canvas::{Frame, Path, Program, Stroke};
use iced::widget::{button, canvas, column, container, row, scrollable, svg, text, text_input};
use iced::{Background, Border, Color, Element, Fill, Length, Theme};

const PANEL_W: f32 = 260.0;
const TOOL_H: f32 = 22.0;
const PANEL_BG: Color = Color { r: 0.13, g: 0.13, b: 0.13, a: 1.0 };
const PANEL_BORDER: Color = Color { r: 0.22, g: 0.22, b: 0.24, a: 1.0 };
const CARD_BG: Color = Color { r: 0.16, g: 0.16, b: 0.18, a: 1.0 };
const CARD_HOVER: Color = Color { r: 0.20, g: 0.20, b: 0.22, a: 1.0 };
const CARD_ACTIVE: Color = Color { r: 0.20, g: 0.40, b: 0.70, a: 1.0 };
const TEXT: Color = Color { r: 0.88, g: 0.88, b: 0.88, a: 1.0 };
const DIM: Color = Color { r: 0.55, g: 0.55, b: 0.55, a: 1.0 };
/// Character budget for a block card's label so all rows stay one line.
/// Fits the narrowest card (Small = 3 per row) at size 11 without wrapping.
const MAX_LABEL_CHARS: usize = 12;
/// Fixed single-line height of a card's label. Clips any text that still
/// won't fit so every cell in a grid row shares the same height.
const LABEL_LINE_H: f32 = 16.0;

/// Every message the block palette emits. Wrapped in `Message::BlockPalette`.
#[derive(Debug, Clone)]
pub enum BlockPaletteMsg {
    Search(String),
    CyclePreviewSize,
    PickFile,
    Insert(String),
    Refresh,
    ToggleBar,
    Close,
    FilePicked(Result<std::path::PathBuf, String>),
}

/// Thumbnail box sizes for the three preview-size settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewSize {
    #[default]
    Small,
    Medium,
    Large,
}

impl PreviewSize {
    /// Number of cards per grid row.
    fn columns(self) -> usize {
        match self {
            PreviewSize::Small => 3,
            PreviewSize::Medium => 2,
            PreviewSize::Large => 1,
        }
    }
    /// Height (px) of the thumbnail box inside each card.
    fn box_height(self) -> f32 {
        match self {
            PreviewSize::Small => 56.0,
            PreviewSize::Medium => 84.0,
            PreviewSize::Large => 120.0,
        }
    }
}

/// Advance to the next preview size (Small → Medium → Large → Small).
pub fn cycle_preview_size(s: PreviewSize) -> PreviewSize {
    match s {
        PreviewSize::Small => PreviewSize::Medium,
        PreviewSize::Medium => PreviewSize::Large,
        PreviewSize::Large => PreviewSize::Small,
    }
}

/// One block's cached preview wires.
pub struct BlockEntry {
    pub name: String,
    pub wires: Vec<WireModel>,
}

/// Panel state held on the app.
#[derive(Default)]
pub struct BlockPalette {
    pub search: String,
    pub preview_size: PreviewSize,
    /// Cached (name, wires) pairs, rebuilt on refresh.
    pub blocks: Vec<BlockEntry>,
    /// Block name currently being placed (INSERT active), for the highlight.
    pub placing: Option<String>,
    /// Last known block-name list, for the cheap stale check.
    pub cached_names: Vec<String>,
    /// Document tab and block-definition revision that produced `blocks`.
    /// Names alone are not enough: separate drawings can share block names,
    /// and a block definition can change without being renamed.
    pub source_tab_id: Option<u64>,
    pub source_block_epoch: u64,
}

/// Canvas program that draws a block's wires fit-to-box inside its bounds.
struct BlockPreviewCanvas<'a> {
    wires: &'a [WireModel],
}

impl<'a> Program<Message> for BlockPreviewCanvas<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let pad = 6.0;
        let inner_w = (bounds.width - 2.0 * pad).max(1.0);
        let inner_h = (bounds.height - 2.0 * pad).max(1.0);
        let (mut minx, mut miny, mut maxx, mut maxy) =
            (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut any = false;
        for w in self.wires {
            for p in &w.points {
                if p[0].is_finite() && p[1].is_finite() {
                    minx = minx.min(p[0]);
                    miny = miny.min(p[1]);
                    maxx = maxx.max(p[0]);
                    maxy = maxy.max(p[1]);
                    any = true;
                }
            }
        }
        if !any {
            // Placeholder box so an empty block still reads as a card.
            let rect = Path::rectangle(iced::Point::new(pad, pad), iced::Size::new(inner_w, inner_h));
            frame.fill(&rect, Color { r: 0.10, g: 0.10, b: 0.10, a: 1.0 });
            return vec![frame.into_geometry()];
        }
        let span_x = (maxx - minx).max(1e-6);
        let span_y = (maxy - miny).max(1e-6);
        let scale = (inner_w / span_x).min(inner_h / span_y);
        let ox = (bounds.width - span_x * scale) * 0.5;
        let oy = (bounds.height - span_y * scale) * 0.5;
        // World up → screen down; centre the fitted AABB.
        let map = |x: f32, y: f32| iced::Point::new(ox + (x - minx) * scale, oy + (maxy - y) * scale);
        // Solid fills first so strokes sit on top.
        for w in self.wires {
            let col = Color { r: w.color[0], g: w.color[1], b: w.color[2], a: 0.55 };
            for tri in w.fill_tris.chunks(3) {
                if tri.len() == 3 {
                    let path = Path::new(|p| {
                        p.move_to(map(tri[0][0], tri[0][1]));
                        p.line_to(map(tri[1][0], tri[1][1]));
                        p.line_to(map(tri[2][0], tri[2][1]));
                        p.close();
                    });
                    frame.fill(&path, col);
                }
            }
        }
        // Polylines, split into runs on NaN separators (NaN = polyline break).
        for w in self.wires {
            let col = Color { r: w.color[0], g: w.color[1], b: w.color[2], a: 1.0 };
            let width = if w.line_weight_px > 1.5 { 2.0 } else { 1.0 };
            let mut run: Vec<iced::Point> = Vec::new();
            let flush = |frame: &mut Frame, run: &mut Vec<iced::Point>, col: Color, width: f32| {
                if run.len() >= 2 {
                    let path = Path::new(|p| {
                        p.move_to(run[0]);
                        for &pt in &run[1..] {
                            p.line_to(pt);
                        }
                    });
                    frame.stroke(&path, Stroke::default().with_color(col).with_width(width));
                }
                run.clear();
            };
            for p in &w.points {
                if p[0].is_finite() && p[1].is_finite() {
                    run.push(map(p[0], p[1]));
                } else {
                    flush(&mut frame, &mut run, col, width);
                }
            }
            flush(&mut frame, &mut run, col, width);
        }
        vec![frame.into_geometry()]
    }
}

/// Build the docked panel element from the palette state.
pub fn view(palette: &BlockPalette) -> Element<'_, Message> {
    let query = palette.search.trim().to_lowercase();
    let filtered: Vec<&BlockEntry> = palette
        .blocks
        .iter()
        .filter(|block| query.is_empty() || block.name.to_lowercase().contains(&query))
        .collect();

    // ── Title bar (title, collapse, close) ──────────────────────────────────
    let title_bar = container(
        row![
            text("Block Palette").size(12).color(TEXT),
            iced::widget::Space::new().width(Fill),
            icon_button(
                IconKind::Svg(include_bytes!("../../../assets/icons/ui/tri_right.svg")),
                BlockPaletteMsg::ToggleBar,
            ),
            icon_button(
                IconKind::Svg(include_bytes!("../../../assets/icons/ui/close.svg")),
                BlockPaletteMsg::Close,
            ),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .style(|_: &Theme| container::Style {
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            color: PANEL_BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .width(Fill)
    .padding([5, 6]);

    let search_input = text_input("Search blocks…", &palette.search)
        .on_input(|v| Message::BlockPalette(BlockPaletteMsg::Search(v)))
        .padding([4, 8])
        .size(12);

    let header = row![
        search_input,
        icon_button(
            IconKind::Svg(include_bytes!("../../../assets/icons/blocks/insert.svg")),
            BlockPaletteMsg::PickFile,
        ),
        icon_button(
            IconKind::Svg(include_bytes!("../../../assets/icons/blocks/preview_size.svg")),
            BlockPaletteMsg::CyclePreviewSize,
        ),
    ]
    .spacing(4)
    .align_y(iced::Center);

    let body: Element<'_, Message> = if filtered.is_empty() {
        let msg = if palette.cached_names.is_empty() {
            "No blocks in this drawing"
        } else {
            "No matches"
        };
        container(text(msg).size(12).color(DIM))
            .center_x(Fill)
            .center_y(Fill)
            .width(Fill)
            .height(Fill)
            .into()
    } else {
        let mut col = column![].spacing(6);
        for chunk in filtered.chunks(palette.preview_size.columns()) {
            let mut r = row![].spacing(6).width(Fill);
            for block in chunk {
                r = r.push(block_card(palette, block));
            }
            col = col.push(r);
        }
        scrollable(
            container(col).padding(iced::Padding {
                top: 0.0,
                right: 8.0,
                bottom: 6.0,
                left: 0.0,
            }),
        )
        .width(Fill)
        .height(Fill)
        .into()
    };

    container(column![title_bar, header, body].spacing(6).padding(6))
        .width(Length::Fixed(PANEL_W))
        .height(Fill)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(PANEL_BG)),
            border: Border {
                color: PANEL_BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn block_card<'a>(palette: &'a BlockPalette, block: &'a BlockEntry) -> Element<'a, Message> {
    let is_placing = palette.placing.as_deref() == Some(block.name.as_str());
    let preview = canvas(BlockPreviewCanvas { wires: &block.wires })
        .width(Fill)
        .height(Length::Fixed(palette.preview_size.box_height()));
    // Fixed height guarantees the label occupies exactly one line, so every
    // cell in a row keeps the same height even for long names (the elide budget
    // handles overflow, and this clips anything that still won't fit).
    let label = text(crate::ui::text_util::elide(&block.name, MAX_LABEL_CHARS))
        .size(11)
        .color(TEXT)
        .width(Fill)
        .height(Length::Fixed(LABEL_LINE_H))
        .center();
    let content = column![preview, label].spacing(4).padding(6);
    button(content)
        .on_press(Message::BlockPalette(BlockPaletteMsg::Insert(block.name.clone())))
        .width(Fill)
        .style(move |_: &Theme, status| {
            let bg = if is_placing {
                CARD_ACTIVE
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => CARD_HOVER,
                    _ => CARD_BG,
                }
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    color: PANEL_BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                text_color: TEXT,
                ..Default::default()
            }
        })
        .into()
}

fn icon_button<'a>(icon: IconKind, msg: BlockPaletteMsg) -> Element<'a, Message> {
    let icon_el: Element<'_, Message> = match icon {
        IconKind::Glyph(s) => text(s).size(15).color(Color::WHITE).into(),
        IconKind::Svg(bytes) => svg(svg::Handle::from_memory(bytes))
            .width(Length::Fixed(TOOL_H))
            .height(Length::Fixed(TOOL_H))
            .into(),
    };
    button(icon_el)
        .on_press(Message::BlockPalette(msg))
        .width(Length::Fixed(TOOL_H + 8.0))
        .height(Length::Fixed(TOOL_H + 8.0))
        .style(|_: &Theme, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered | button::Status::Pressed => CARD_HOVER,
                _ => Color::TRANSPARENT,
            })),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            text_color: Color::WHITE,
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_size_cycles() {
        assert_eq!(cycle_preview_size(PreviewSize::Small), PreviewSize::Medium);
        assert_eq!(cycle_preview_size(PreviewSize::Medium), PreviewSize::Large);
        assert_eq!(cycle_preview_size(PreviewSize::Large), PreviewSize::Small);
    }
}
