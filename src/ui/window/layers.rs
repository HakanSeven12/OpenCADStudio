//! Layer Manager — floating window.

use crate::app::Message;
use crate::ui::properties::{lw_options, LinetypeItem, LwItem};
use crate::ui::ROW_H;
use acadrust::tables::layer::Layer as DocLayer;
use acadrust::tables::Table;
use acadrust::types::aci_table::aci_to_rgb;
use acadrust::types::{Color as AcadColor, LineWeight};
use acadrust::Handle;
use iced::widget::{
    button, column, combo_box, container, mouse_area, row, scrollable, text, text_input, tooltip,
};
use iced::Padding;
use iced::{Background, Border, Color, Element, Fill, Length, Theme};

// ── Per-viewport column descriptor ───────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct VpCol {
    pub handle: Handle,
    pub label: String,
}

/// Sortable column in the Layer Manager table. Clicking a header sorts by
/// that column; clicking the active header again flips the direction (#133).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerSortCol {
    Name,
    On,
    Freeze,
    Lock,
    Color,
    Linetype,
    Lineweight,
    Transparency,
}

// ── Row-height-derived constants ─────────────────────────────────────────
/// SVG icon size inside a layer-table cell.
const ICON_SZ: f32 = ROW_H * 0.62; // ≈16 px at ROW_H=26
/// Font size for cell text.
const FONT_SZ: f32 = ROW_H * 0.42; // ≈11 px at ROW_H=26
/// Vertical padding for combo_box / text_input so their total height = ROW_H.
const COMBO_PAD_V: f32 = (ROW_H - FONT_SZ * 1.3 - 2.0) / 2.0;
/// Widget id for the layer-table scrollable, so a freshly created layer can be
/// scrolled into view after it is added (#271).
pub const LAYER_TABLE_SCROLL_ID: &str = "layer-manager-table-scroll";

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

fn table_input_style(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let palette = theme.extended_palette();
    let border = match status {
        iced::widget::text_input::Status::Focused { .. } => palette.primary.base.color,
        _ => palette.background.neutral.color,
    };
    iced::widget::text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border {
            color: border,
            width: 1.0,
            radius: 2.0.into(),
        },
        icon: palette.background.base.text,
        placeholder: palette.background.base.text.scale_alpha(0.48),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.5),
    }
}

// ── Layer data ────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub frozen: bool,
    pub locked: bool,
    pub color: Color,
    pub linetype: String,
    pub lineweight: LineWeight,
    pub transparency: i32,
    /// Freeze state per-viewport, indexed parallel to LayerPanel::vp_cols.
    pub vp_frozen: Vec<bool>,
}

impl Layer {
    pub fn new(name: &str, color: Color) -> Self {
        Self {
            name: name.to_string(),
            visible: true,
            frozen: false,
            locked: false,
            color,
            linetype: "Continuous".to_string(),
            lineweight: LineWeight::Default,
            transparency: 0,
            vp_frozen: vec![],
        }
    }
}

// ── Panel state ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct LayerPanel {
    pub layers: Vec<Layer>,
    #[allow(dead_code)]
    pub visible: bool,
    /// Anchor row: the last row clicked. Drives the editable combos and the
    /// Shift-range origin. Its layer is always part of `selected_multi`.
    pub selected: Option<usize>,
    /// All selected rows (Ctrl/Shift extend it). Bulk property changes and
    /// deletion act on every row here. Empty ⇔ nothing selected.
    pub selected_multi: Vec<usize>,
    pub editing: Option<usize>,
    pub edit_buf: String,
    pub current_layer: String,
    pub linetype_items: Vec<LinetypeItem>,
    pub color_picker_row: Option<usize>,
    pub color_full_palette: bool,
    pub linetype_combo: combo_box::State<LinetypeItem>,
    pub lw_combo: combo_box::State<LwItem>,
    /// Per-viewport columns (only populated when in a paper layout with viewports).
    pub vp_cols: Vec<VpCol>,
    /// Active sort column, or `None` for document order.
    pub sort_col: Option<LayerSortCol>,
    /// Sort direction; `true` = ascending.
    pub sort_asc: bool,
    /// Live name filter from the search box; empty shows every layer (#343).
    pub filter: String,
}

impl Default for LayerPanel {
    fn default() -> Self {
        Self {
            visible: false,
            layers: vec![Layer::new("0", Color::WHITE)],
            selected: None,
            selected_multi: Vec::new(),
            editing: None,
            edit_buf: String::new(),
            current_layer: "0".to_string(),
            linetype_items: vec![LinetypeItem {
                name: "Continuous".into(),
                art: String::new(),
            }],
            color_picker_row: None,
            color_full_palette: false,
            linetype_combo: combo_box::State::new(vec![LinetypeItem {
                name: "Continuous".into(),
                art: String::new(),
            }]),
            lw_combo: combo_box::State::new(lw_options()),
            vp_cols: vec![],
            // Default to alphabetical (Name) order in both the Layer Manager
            // table and the ribbon's quick layer dropdown (#270). A header
            // click still re-sorts by any other column.
            sort_col: Some(LayerSortCol::Name),
            sort_asc: true,
            filter: String::new(),
        }
    }
}

impl LayerPanel {
    /// Sync layers + update per-viewport freeze columns.
    /// `vp_info`: list of (vp_handle, vp_label, frozen_layer_handles) from scene.
    pub fn sync_with_viewports(
        &mut self,
        doc_layers: &Table<DocLayer>,
        vp_info: Vec<(Handle, String, Vec<Handle>)>,
    ) {
        // The rebuild below re-indexes rows, so capture the selection by name
        // first and re-resolve it after (indices alone would go stale).
        let anchor_name = self
            .selected
            .and_then(|i| self.layers.get(i))
            .map(|l| l.name.clone());
        let multi_names: Vec<String> = self
            .selected_multi
            .iter()
            .filter_map(|&i| self.layers.get(i).map(|l| l.name.clone()))
            .collect();

        self.vp_cols = vp_info
            .iter()
            .map(|(h, label, _)| VpCol {
                handle: *h,
                label: label.clone(),
            })
            .collect();

        self.layers = doc_layers
            .iter()
            .map(|l| {
                let layer_handle = l.handle;
                let vp_frozen = vp_info
                    .iter()
                    .map(|(_, _, frozen_handles)| frozen_handles.contains(&layer_handle))
                    .collect();
                Layer {
                    name: l.name.clone(),
                    visible: !l.flags.off,
                    frozen: l.flags.frozen,
                    locked: l.flags.locked,
                    color: iced_color_from_acad(&l.color),
                    linetype: if l.line_type.is_empty() {
                        "Continuous".to_string()
                    } else {
                        l.line_type.clone()
                    },
                    lineweight: l.line_weight,
                    transparency: 0,
                    vp_frozen,
                }
            })
            .collect();

        // Re-resolve the selection against the rebuilt rows by name.
        self.selected = anchor_name
            .and_then(|n| self.layers.iter().position(|l| l.name == n));
        self.selected_multi = multi_names
            .iter()
            .filter_map(|n| self.layers.iter().position(|l| l.name == *n))
            .collect();

        self.apply_sort();
    }

    /// Set/flip the sort column from a header click, then re-sort.
    pub fn sort_by(&mut self, col: LayerSortCol) {
        if self.sort_col == Some(col) {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort_col = Some(col);
            self.sort_asc = true;
        }
        self.apply_sort();
    }

    /// Reorder `self.layers` by the active sort column, preserving the current
    /// selection by name. No-op in document order (`sort_col == None`).
    fn apply_sort(&mut self) {
        let Some(col) = self.sort_col else {
            return;
        };
        let asc = self.sort_asc;
        let sel_name = self
            .selected
            .and_then(|i| self.layers.get(i))
            .map(|l| l.name.clone());
        // Sort reorders rows, so the multi-selection (stored as indices) must
        // be re-resolved by name afterward or it would point at the wrong rows.
        let multi_names: Vec<String> = self
            .selected_multi
            .iter()
            .filter_map(|&i| self.layers.get(i).map(|l| l.name.clone()))
            .collect();

        use std::cmp::Ordering;
        self.layers.sort_by(|a, b| {
            let ord = match col {
                LayerSortCol::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                LayerSortCol::On => a.visible.cmp(&b.visible),
                LayerSortCol::Freeze => a.frozen.cmp(&b.frozen),
                LayerSortCol::Lock => a.locked.cmp(&b.locked),
                LayerSortCol::Color => color_sort_key(a.color).cmp(&color_sort_key(b.color)),
                LayerSortCol::Linetype => {
                    a.linetype.to_lowercase().cmp(&b.linetype.to_lowercase())
                }
                LayerSortCol::Lineweight => a.lineweight.value().cmp(&b.lineweight.value()),
                LayerSortCol::Transparency => a.transparency.cmp(&b.transparency),
            };
            // Stable tie-break by name so equal keys keep a predictable order.
            let ord = ord.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            if asc {
                ord
            } else {
                match ord {
                    Ordering::Less => Ordering::Greater,
                    Ordering::Greater => Ordering::Less,
                    Ordering::Equal => Ordering::Equal,
                }
            }
        });

        if let Some(n) = sel_name {
            self.selected = self.layers.iter().position(|l| l.name == n);
        }
        self.selected_multi = multi_names
            .iter()
            .filter_map(|n| self.layers.iter().position(|l| l.name == *n))
            .collect();
    }

    pub fn sync_linetypes(&mut self, items: Vec<LinetypeItem>) {
        self.linetype_combo = combo_box::State::new(items.clone());
        self.linetype_items = items;
    }

    /// Render the layer panel as the full content of its own OS window.
    pub fn view_window(&self, name_col_w: f32) -> Element<'_, Message> {
        self.view_content(name_col_w)
    }

    fn view_content(&self, name_col_w: f32) -> Element<'_, Message> {
        let has_sel = self.selected.is_some();
        let sel_is_zero = self
            .selected
            .map(|i| self.layers.get(i).map(|l| l.name == "0").unwrap_or(false))
            .unwrap_or(false);

        // ── Toolbar ───────────────────────────────────────────────────────
        let toolbar = container(
            row![
                toolbar_btn(crate::ui::icons::PLUS, "New", Message::LayerNew),
                toolbar_btn_cond(
                    crate::ui::icons::TRASH,
                    "Delete",
                    Message::LayerDelete,
                    has_sel && !sel_is_zero,
                ),
                toolbar_btn_cond(
                    crate::ui::icons::CHECK,
                    "Set Current",
                    Message::LayerSetCurrent,
                    has_sel,
                ),
                iced::widget::Space::new().width(Fill),
                // Search box: filters rows by name as the user types (#343).
                text_input("Search…", &self.filter)
                    .on_input(Message::LayerManagerFilterChanged)
                    .size(FONT_SZ)
                    .padding([3, 6])
                    .width(Length::Fixed(180.0))
                    .style(table_input_style),
            ]
            .spacing(2)
            .align_y(iced::Center),
        )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.weak.color
            )),
            ..Default::default()
        })
        .width(Fill)
        .padding([4, 8]);

        // ── Column header ─────────────────────────────────────────────────
        let sc = self.sort_col;
        let sa = self.sort_asc;
        let mut header_row = row![
            text("Status").size(10).style(muted_style).width(50),
            sortable_header("Name", LayerSortCol::Name, Length::Fixed(name_col_w), sc, sa),
            // Draggable divider: adjusts the Name column width (#359).
            iced::widget::mouse_area(
                container(iced::widget::Space::new().width(2).height(14)).style(
                    |theme: &Theme| container::Style {
                        background: Some(Background::Color(
                            theme.extended_palette().background.neutral.color
                        )),
                        ..Default::default()
                    },
                ),
            )
            .on_press(Message::LayerNameColGrab)
            .interaction(iced::mouse::Interaction::ResizingHorizontally),
            sortable_header("On", LayerSortCol::On, Length::Fixed(COL_ICON), sc, sa),
            sortable_header("Freeze", LayerSortCol::Freeze, Length::Fixed(COL_ICON), sc, sa),
            sortable_header("Lock", LayerSortCol::Lock, Length::Fixed(COL_ICON), sc, sa),
            sortable_header("Color", LayerSortCol::Color, Length::Fixed(COL_COLOR), sc, sa),
            sortable_header("Linetype", LayerSortCol::Linetype, Length::Fixed(COL_LT), sc, sa),
            sortable_header("Lineweight", LayerSortCol::Lineweight, Length::Fixed(COL_LW), sc, sa),
            sortable_header(
                "Transparency",
                LayerSortCol::Transparency,
                Length::Fixed(COL_TRANS),
                sc,
                sa
            ),
        ]
        .spacing(4)
        .width(Fill)
        .align_y(iced::Center);

        for vp in &self.vp_cols {
            header_row = header_row.push(
                text(vp.label.as_str())
                    .size(10)
                    .style(muted_style)
                    .width(Length::Fixed(COL_ICON)),
            );
        }

        let col_header = container(header_row)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
                }
            })
            .padding([4, 8])
            .width(Fill);

        // ── Layer rows ────────────────────────────────────────────────────
        let mut rows_col = column![].spacing(0);
        let filter = self.filter.to_lowercase();
        for (i, layer) in self.layers.iter().enumerate() {
            if !filter.is_empty() && !layer.name.to_lowercase().contains(&filter) {
                continue;
            }
            // Highlight every selected row; show the editable combos only on the
            // anchor (a single shared combo state can't drive several rows).
            let is_anchor = self.selected == Some(i);
            let is_sel = is_anchor || self.selected_multi.contains(&i);
            let is_current = layer.name == self.current_layer;
            let is_editing = self.editing == Some(i);
            let color_open = self.color_picker_row == Some(i);

            let (ltc, lwc) = if is_anchor {
                (Some(&self.linetype_combo), Some(&self.lw_combo))
            } else {
                (None, None)
            };

            rows_col = rows_col.push(layer_row(
                i,
                layer,
                is_sel,
                is_current,
                is_editing,
                &self.edit_buf,
                color_open,
                ltc,
                lwc,
                &self.vp_cols,
                name_col_w,
            ));

        }

        let table = scrollable(rows_col)
            .id(iced::advanced::widget::Id::new(LAYER_TABLE_SCROLL_ID))
            .height(Fill);

        // ── Full-window frame ─────────────────────────────────────────────
        container(column![toolbar, col_header, table].spacing(0))
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme.extended_palette().background.base.color
                )),
                ..Default::default()
            })
            .width(Fill)
            .height(Fill)
            .into()
    }
}

// ── Sorting helpers ─────────────────────────────────────────────────────────

fn layer_cell_button_style(
    theme: &Theme,
    status: button::Status,
    is_selected: bool,
    index: usize,
) -> button::Style {
    let palette = theme.extended_palette();
    let highlighted = matches!(status, button::Status::Hovered);
    let pair = if highlighted {
        palette.background.strong
    } else if is_selected {
        palette.primary.weak
    } else if index % 2 == 0 {
        palette.background.base
    } else {
        palette.background.weak
    };
    button::Style {
        background: highlighted.then_some(Background::Color(pair.color)),
        text_color: pair.text,
        ..Default::default()
    }
}

fn layer_header_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let highlighted = matches!(
        status,
        button::Status::Hovered | button::Status::Pressed
    );
    let pair = if highlighted {
        palette.background.strong
    } else {
        palette.background.weak
    };
    button::Style {
        background: highlighted.then_some(Background::Color(pair.color)),
        text_color: pair.text,
        ..Default::default()
    }
}

/// Packed RGB key for ordering colours deterministically by hue-ish bytes.
fn color_sort_key(c: Color) -> u32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (q(c.r) << 16) | (q(c.g) << 8) | q(c.b)
}

/// A clickable column header that sorts the table by `col`. Shows an up/down
/// SVG arrow when it is the active sort column (#133).
fn sortable_header<'a>(
    label: &'a str,
    col: LayerSortCol,
    width: Length,
    active: Option<LayerSortCol>,
    asc: bool,
) -> Element<'a, Message> {
    let mut content = row![text(label).size(10).style(muted_style)]
        .spacing(3)
        .align_y(iced::Center);
    if active == Some(col) {
        content = content.push(if asc {
            crate::ui::icons::themed_arrow_up(8.0)
        } else {
            crate::ui::icons::themed_arrow_down(8.0)
        });
    }
    button(content)
        .on_press(Message::LayerSort(col))
        .style(layer_header_button_style)
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: 2.0,
            right: 2.0,
        })
        .width(width)
        .into()
}

// ── Toolbar buttons ───────────────────────────────────────────────────────

fn toolbar_btn<'a>(icon: &'static [u8], label: &'a str, msg: Message) -> Element<'a, Message> {
    button(
        row![
            crate::ui::icons::themed(icon, 12.0),
            text(label).size(11),
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .on_press(msg)
        .style(|theme: &Theme, status| {
            let palette = theme.extended_palette();
            let pair = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    palette.background.strong
                }
                _ => palette.background.weak,
            };
            button::Style {
            background: Some(Background::Color(pair.color)),
            border: Border {
                radius: 3.0.into(),
                color: palette.background.neutral.color,
                width: 1.0,
            },
            text_color: pair.text,
            ..Default::default()
            }
        })
        .padding([4, 10])
        .into()
}

fn toolbar_btn_cond<'a>(
    icon: &'static [u8],
    label: &'a str,
    msg: Message,
    enabled: bool,
) -> Element<'a, Message> {
    let mut b = button(
        row![
            if enabled {
                crate::ui::icons::themed(icon, 12.0)
            } else {
                crate::ui::icons::themed_disabled(icon, 12.0)
            },
            if enabled {
                text(label).size(11)
            } else {
                text(label).size(11).style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(
                        theme.extended_palette().background.base.text.scale_alpha(0.42)
                    ),
                })
            },
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .style(move |theme: &Theme, status| {
        let palette = theme.extended_palette();
        let pair = match status {
            button::Status::Hovered if enabled => palette.background.strong,
            _ => palette.background.weak,
        };
        button::Style {
        background: Some(Background::Color(pair.color)),
        border: Border {
            radius: 3.0.into(),
            color: palette.background.neutral.color,
            width: 1.0,
        },
        text_color: if enabled {
            pair.text
        } else {
            pair.text.scale_alpha(0.42)
        },
        ..Default::default()
        }
    })
    .padding([4, 10]);
    if enabled {
        b = b.on_press(msg);
    }
    b.into()
}

// ── Layer row ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
/// Hover popup showing a layer's full name when the cell truncates it.
fn name_tip<'a>(name: &'a str) -> Element<'a, Message> {
    container(text(name).size(FONT_SZ))
        .padding(Padding {
            top: 3.0,
            bottom: 3.0,
            left: 7.0,
            right: 7.0,
        })
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
            background: Some(Background::Color(palette.background.strong.color)),
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: Some(palette.background.strong.text),
            ..Default::default()
            }
        })
        .into()
}

fn layer_row<'a>(
    index: usize,
    layer: &'a Layer,
    is_selected: bool,
    is_current: bool,
    is_editing: bool,
    edit_buf: &'a str,
    color_picker_open: bool,
    lt_combo: Option<&'a combo_box::State<LinetypeItem>>,
    lw_combo_state: Option<&'a combo_box::State<LwItem>>,
    vp_cols: &'a [VpCol],
    name_col_w: f32,
) -> Element<'a, Message> {
    let svg_btn = |bytes: &'static [u8], on_press: Message| -> Element<'a, Message> {
        button(crate::ui::icons::semantic(bytes, ICON_SZ))
        .on_press(on_press)
        .style(move |theme: &Theme, status| {
            layer_cell_button_style(theme, status, is_selected, index)
        })
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 4.0,
            right: 4.0,
        })
        .height(Length::Fixed(ROW_H))
        .into()
    };

    let vis_svg = crate::ui::icons::layer_visible(layer.visible);
    let frz_svg = crate::ui::icons::layer_freeze(layer.frozen);
    let lck_svg = crate::ui::icons::layer_lock(layer.locked);

    let status_dot: Element<'_, Message> = if is_current {
        crate::ui::icons::themed_success(crate::ui::icons::CHECK, 13.0)
    } else {
        crate::ui::icons::themed_secondary(crate::ui::icons::DOT, 9.0)
    };

    // Name cell
    let name_cell: Element<'_, Message> = if is_editing {
        text_input("", edit_buf)
            .on_input(Message::LayerRenameEdit)
            .on_submit(Message::LayerRenameCommit)
            .size(FONT_SZ)
            .padding(Padding {
                top: COMBO_PAD_V,
                bottom: COMBO_PAD_V,
                left: 4.0,
                right: 4.0,
            })
            .style(table_input_style)
            .width(Length::Fixed(name_col_w))
            .into()
    } else {
        // ~6 px per glyph at the 10 px row font — track the column width.
        let name_budget = ((name_col_w / 6.0) as usize).max(8);
        let name_btn = button(
            text(crate::ui::text_util::elide(&layer.name, name_budget))
                .size(FONT_SZ),
        )
        .on_press(Message::LayerRenameStart(index))
        .style(move |theme: &Theme, status| {
            layer_cell_button_style(theme, status, is_selected, index)
        })
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 4.0,
            right: 4.0,
        })
        .height(Length::Fixed(ROW_H))
        .width(Length::Fixed(name_col_w));
        // When the name is truncated, reveal the full text on hover so the
        // user can still read it without widening the column.
        if layer.name.chars().count() > name_budget {
            tooltip(name_btn, name_tip(&layer.name), tooltip::Position::FollowCursor).into()
        } else {
            name_btn.into()
        }
    };

    // Color cell — looks like a combo_box input; click opens swatch dropdown below row.
    let aci = iced_to_aci(layer.color);
    let cur_color_name = color_label_aci(aci).to_string();
    let _ = cur_color_name;
    // Shared colour selector. Layers carry a concrete colour (no ByLayer /
    // ByBlock); the chosen index is applied to this row.
    let color_cell: Element<'_, Message> = container(crate::ui::color_select::color_selector(
        acadrust::types::Color::Index(aci),
        color_picker_open,
        crate::ui::color_select::ColorExtras {
            by_layer: false,
            by_block: false,
        },
        |c| match c {
            acadrust::types::Color::Index(i) => Message::LayerColorSet(i),
            _ => Message::LayerColorSet(7),
        },
        Message::LayerColorPickerToggle(index),
        Message::OpenColorWindow(crate::app::ColorPickTarget::Layer(index)),
    ))
    .width(Length::Fixed(COL_COLOR))
    .into();

    // Linetype cell — uses LinetypeItem (with ASCII art) same as Properties panel
    let cur_lt_item = LinetypeItem {
        name: layer.linetype.clone(),
        art: String::new(), // art comes from combo state items; just match by name
    };
    let lt_cell: Element<'_, Message> = if let Some(state) = lt_combo {
        combo_box(
            state,
            "linetype",
            Some(&cur_lt_item),
            |item: LinetypeItem| Message::LayerLinetypeSet(item.name),
        )
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 4.0,
            right: 4.0,
        })
        .width(Length::Fixed(COL_LT))
        .input_style(combo_input_style)
        .into()
    } else {
        text(layer.linetype.as_str())
            .size(FONT_SZ)
            .style(muted_style)
            .width(Length::Fixed(COL_LT))
            .into()
    };

    // Lineweight cell
    let cur_lw_item = LwItem(layer.lineweight);
    let lw_cell: Element<'_, Message> = if let Some(state) = lw_combo_state {
        combo_box(state, "lineweight", Some(&cur_lw_item), |item: LwItem| {
            Message::LayerLineweightSet(item.0)
        })
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 4.0,
            right: 4.0,
        })
        .width(Length::Fixed(COL_LW))
        .input_style(combo_input_style)
        .into()
    } else {
        text(cur_lw_item.to_string())
            .size(FONT_SZ)
            .style(muted_style)
            .width(Length::Fixed(COL_LW))
            .into()
    };

    // Transparency cell
    let trans_str = layer.transparency.to_string();
    let trans_cell = text_input("0", &trans_str)
        .on_input(move |s| Message::LayerTransparencyEdit(index, s))
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 4.0,
            right: 4.0,
        })
        .style(|theme: &Theme, status| {
            let mut style = table_input_style(theme, status);
            style.background = iced::Background::Color(Color::TRANSPARENT);
            style
        })
        .width(Length::Fixed(COL_TRANS));

    let mut row_content = row![
        container(status_dot)
            .width(50)
            .align_x(iced::Center)
            .align_y(iced::Center),
        name_cell,
        iced::widget::Space::new().width(2),
        container(svg_btn(vis_svg, Message::LayerToggleVisible(index)))
            .width(Length::Fixed(COL_ICON))
            .align_x(iced::Center),
        container(svg_btn(frz_svg, Message::LayerToggleFreeze(index)))
            .width(Length::Fixed(COL_ICON))
            .align_x(iced::Center),
        container(svg_btn(lck_svg, Message::LayerToggleLock(index)))
            .width(Length::Fixed(COL_ICON))
            .align_x(iced::Center),
        color_cell,
        lt_cell,
        lw_cell,
        trans_cell,
    ]
    .spacing(4)
    .width(Fill)
    .align_y(iced::Center);

    // Per-viewport freeze columns
    for (vp_idx, _vp_col) in vp_cols.iter().enumerate() {
        let is_vp_frozen = layer.vp_frozen.get(vp_idx).copied().unwrap_or(false);
        let vp_frz_svg = crate::ui::icons::layer_freeze(is_vp_frozen);
        row_content = row_content.push(
            container(svg_btn(
                vp_frz_svg,
                Message::LayerToggleVpFreeze(index, vp_idx),
            ))
            .width(Length::Fixed(COL_ICON))
            .align_x(iced::Center),
        );
    }

    mouse_area(
        container(row_content)
            .style(move |theme: &Theme| {
                let palette = theme.extended_palette();
                let pair = if is_selected {
                    palette.primary.weak
                } else if index % 2 == 0 {
                    palette.background.base
                } else {
                    palette.background.weak
                };
                container::Style {
                background: Some(Background::Color(pair.color)),
                text_color: Some(pair.text),
                ..Default::default()
                }
            })
            .padding(Padding {
                top: 0.0,
                bottom: 0.0,
                left: 8.0,
                right: 8.0,
            })
            .height(Length::Fixed(ROW_H))
            .width(Fill),
    )
    .on_press(Message::LayerSelect(index))
    .into()
}

// ── Combo style ────────────────────────────────────────────────────────────

fn combo_input_style(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    table_input_style(theme, status)
}

// ── Display helpers ───────────────────────────────────────────────────────

#[allow(dead_code)]
fn aci_color_display(i: u8) -> (Color, &'static str) {
    let (r, g, b) = aci_to_rgb(i).unwrap_or((200, 200, 200));
    (
        Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0),
        "",
    )
}

fn iced_to_aci(c: Color) -> u8 {
    let r = (c.r * 255.0) as u8;
    let g = (c.g * 255.0) as u8;
    let b = (c.b * 255.0) as u8;
    for i in 1u8..=255 {
        if let Some((ar, ag, ab)) = aci_to_rgb(i) {
            if ar == r && ag == g && ab == b {
                return i;
            }
        }
    }
    7
}

fn color_label_aci(i: u8) -> &'static str {
    match i {
        1 => "red",
        2 => "yellow",
        3 => "green",
        4 => "cyan",
        5 => "blue",
        6 => "magenta",
        7 => "white",
        8 => "dark gray",
        9 => "gray",
        _ => "white",
    }
}

pub fn iced_color_from_acad(c: &AcadColor) -> Color {
    match c {
        AcadColor::Index(i) => {
            let (r, g, b) = aci_to_rgb(*i).unwrap_or((200, 200, 200));
            Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
        }
        AcadColor::Rgb { r, g, b } => {
            Color::from_rgb(*r as f32 / 255.0, *g as f32 / 255.0, *b as f32 / 255.0)
        }
        _ => Color::WHITE,
    }
}

// ── Column widths ─────────────────────────────────────────────────────────

// Name column width is user-adjustable via the header divider drag (#359);
// the app passes the current width into `view_window`.
const COL_ICON: f32 = 44.0;
const COL_COLOR: f32 = 90.0;
const COL_LT: f32 = 110.0;
const COL_LW: f32 = 90.0;
const COL_TRANS: f32 = 80.0;
