//! Layer Manager — floating window.

use crate::app::Message;
use crate::t;
use crate::ui::properties::{lw_options, LinetypeItem, LwItem};
use crate::ui::style::common::muted_style;
use crate::ui::style::form::field_style_with_radius;
use crate::ui::style::row_button::{cell_button_style, header_button_style};
use crate::ui::ROW_H;
use document::tables::layer::Layer as DocLayer;
use document::tables::Table;
use document::types::{Color as AcadColor, LineWeight};
use document::Handle;
use iced::widget::{
    button, checkbox, column, combo_box, container, mouse_area, row, scrollable, text, text_input,
    tooltip,
};
use iced::Padding;
use iced::{Background, Border, Color, Element, Fill, Length, Theme};
use std::borrow::Cow;

// ── Per-viewport column descriptor ───────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct VpCol {
    pub handle: Handle,
    pub label: String,
}

/// A layout viewport as the Layer Manager shows it: its frozen layers and
/// its layer colour overrides.
pub struct ViewportLayers {
    pub handle: Handle,
    pub label: String,
    pub frozen: Vec<Handle>,
    pub colors: Vec<(Handle, AcadColor)>,
}

/// Sortable column in the Layer Manager table. Clicking a header sorts by
/// that column; clicking the active header again flips the direction (#133).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerSortCol {
    Name,
    On,
    Freeze,
    Lock,
    Plot,
    Color,
    Linetype,
    Lineweight,
    Transparency,
}

impl LayerSortCol {
    /// The name a column is saved under.
    fn key(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::On => "On",
            Self::Freeze => "Freeze",
            Self::Lock => "Lock",
            Self::Plot => "Plot",
            Self::Color => "Color",
            Self::Linetype => "Linetype",
            Self::Lineweight => "Lineweight",
            Self::Transparency => "Transparency",
        }
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            Self::Name => t!("Name"),
            Self::On => t!("On"),
            Self::Freeze => t!("Freeze"),
            Self::Lock => t!("Lock"),
            Self::Plot => t!("Plot"),
            Self::Color => t!("Color"),
            Self::Linetype => t!("Linetype"),
            Self::Lineweight => t!("Lineweight"),
            Self::Transparency => t!("Transparency"),
        }
    }

    fn width(self) -> f32 {
        match self {
            Self::Color => COL_COLOR,
            Self::Linetype => COL_LT,
            Self::Lineweight => COL_LW,
            Self::Transparency => COL_TRANS,
            _ => COL_ICON,
        }
    }
}

/// The columns after Name that can be moved and hidden, in their default
/// order.
const COLUMNS: [LayerSortCol; 8] = [
    LayerSortCol::On,
    LayerSortCol::Freeze,
    LayerSortCol::Lock,
    LayerSortCol::Plot,
    LayerSortCol::Color,
    LayerSortCol::Linetype,
    LayerSortCol::Lineweight,
    LayerSortCol::Transparency,
];

thread_local! {
    /// The columns' order and which show; a user preference (UI thread).
    static COLUMN_LAYOUT: std::cell::RefCell<Vec<(LayerSortCol, bool)>> =
        std::cell::RefCell::new(COLUMNS.iter().map(|&col| (col, true)).collect());
}

pub fn column_layout() -> Vec<(LayerSortCol, bool)> {
    COLUMN_LAYOUT.with(|layout| layout.borrow().clone())
}

/// The layout as saved: column names in order, a hidden one led by `-`.
pub fn column_layout_names() -> Vec<String> {
    column_layout()
        .into_iter()
        .map(|(col, shown)| format!("{}{}", if shown { "" } else { "-" }, col.key()))
        .collect()
}

/// The saved layout back; unknown names are skipped and columns the save
/// lacks come last, shown.
pub fn set_column_layout_names(names: &[String]) {
    let mut layout: Vec<(LayerSortCol, bool)> = names
        .iter()
        .filter_map(|name| {
            let (shown, key) = match name.strip_prefix('-') {
                Some(key) => (false, key),
                None => (true, name.as_str()),
            };
            COLUMNS.iter().find(|col| col.key() == key).map(|&col| (col, shown))
        })
        .collect();
    for col in COLUMNS {
        if !layout.iter().any(|(c, _)| *c == col) {
            layout.push((col, true));
        }
    }
    layout.dedup_by_key(|(col, _)| *col);
    COLUMN_LAYOUT.with(|current| *current.borrow_mut() = layout);
}

/// Show or hide `col`.
pub fn toggle_column(col: LayerSortCol) {
    COLUMN_LAYOUT.with(|layout| {
        if let Some(entry) = layout.borrow_mut().iter_mut().find(|(c, _)| *c == col) {
            entry.1 = !entry.1;
        }
    });
}

/// Move `col` one place left (`-1`) or right (`1`).
pub fn move_column(col: LayerSortCol, step: i8) {
    COLUMN_LAYOUT.with(|layout| {
        let mut layout = layout.borrow_mut();
        let Some(at) = layout.iter().position(|(c, _)| *c == col) else {
            return;
        };
        let to = (at as i64 + i64::from(step)).clamp(0, layout.len() as i64 - 1) as usize;
        layout.swap(at, to);
    });
}

// ── Row-height-derived constants ─────────────────────────────────────────
/// SVG icon size inside a layer-table cell.
const ICON_SZ: f32 = ROW_H * 0.62; // ≈16 px at ROW_H=26
/// Font size for cell text.
const FONT_SZ: f32 = ROW_H * 0.42; // ≈11 px at ROW_H=26
/// Vertical padding for combo_box / text_input so their total height = ROW_H.
const COMBO_PAD_V: f32 = (ROW_H - FONT_SZ * 1.3 - 2.0) / 2.0;
const LINETYPE_MENU_W: f32 = 220.0;
/// Widget id for the layer-table scrollable, so a freshly created layer can be
/// scrolled into view after it is added (#271).
pub const LAYER_TABLE_SCROLL_ID: &str = "layer-manager-table-scroll";

// ── Layer data ────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub frozen: bool,
    pub locked: bool,
    pub plottable: bool,
    pub color: AcadColor,
    pub color_name: Option<String>,
    pub book_name: Option<String>,
    pub linetype: String,
    pub lineweight: LineWeight,
    pub transparency: i32,
    /// Freeze state per-viewport, indexed parallel to LayerPanel::vp_cols.
    pub vp_frozen: Vec<bool>,
    /// Colour override per-viewport, parallel to `vp_frozen`.
    pub vp_colors: Vec<Option<AcadColor>>,
}

impl Layer {
    pub fn new(name: &str, color: AcadColor) -> Self {
        Self {
            name: name.to_string(),
            visible: true,
            frozen: false,
            locked: false,
            plottable: true,
            color,
            color_name: None,
            book_name: None,
            linetype: "Continuous".to_string(),
            lineweight: LineWeight::Default,
            transparency: 0,
            vp_frozen: vec![],
            vp_colors: vec![],
        }
    }
}

// ── Panel state ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct LayerPanel {
    pub layers: Vec<Layer>,
    /// Anchor row: the last row clicked. Drives the editable combos and the
    /// Shift-range origin. Its layer is always part of `selected_multi`.
    pub selected: Option<usize>,
    /// All selected rows (Ctrl/Shift extend it). Bulk property changes and
    /// deletion act on every row here. Empty ⇔ nothing selected.
    pub selected_multi: Vec<usize>,
    /// Layer being renamed (row text-input open). Keyed by NAME so a
    /// rebuild/sort between start and commit cannot retarget it (#44);
    /// `selected`/`selected_multi` below stay index-based and are re-resolved.
    pub editing: Option<String>,
    pub edit_buf: String,
    pub current_layer: String,
    pub linetype_items: Vec<LinetypeItem>,
    /// Row whose color swatch is open. Name-keyed for the same reason (#44).
    pub color_picker_row: Option<String>,
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
    /// The column chooser (a right-click on the header) is open; it stays
    /// open while columns are ticked, until Close.
    pub columns_menu: bool,
    /// The drawing's layer groups by layer name.
    pub groups: Vec<(String, Vec<String>)>,
    /// The group shown, `None` for every layer.
    pub group: Option<String>,
}

impl Default for LayerPanel {
    fn default() -> Self {
        Self {
            layers: vec![Layer::new("0", AcadColor::Index(7))],
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
            columns_menu: false,
            groups: Vec::new(),
            group: None,
        }
    }
}

impl LayerPanel {
    /// Take the drawing's groups; a shown group that is gone shows all.
    pub fn set_groups(&mut self, groups: Vec<(String, Vec<String>)>) {
        if let Some(shown) = &self.group {
            if !groups.iter().any(|(name, _)| name == shown) {
                self.group = None;
            }
        }
        self.groups = groups;
    }

    /// The selected rows' layer names.
    pub fn selected_names(&self) -> Vec<String> {
        let mut rows = self.selected_multi.clone();
        rows.extend(self.selected);
        rows.sort_unstable();
        rows.dedup();
        rows.into_iter()
            .filter_map(|row| self.layers.get(row).map(|layer| layer.name.clone()))
            .collect()
    }

    /// The group bar under the toolbar: the group shown, a new group from
    /// the selection, adding the selection to a group, taking it out of the
    /// shown group and deleting that group.
    fn group_bar(&self) -> Element<'_, Message> {
        let all = crate::tr!("layer-groups", "all-layers");
        let mut shown_options = vec![all.clone()];
        shown_options.extend(self.groups.iter().map(|(name, _)| name.clone()));
        let shown = self.group.clone().unwrap_or_else(|| all.clone());
        let has_selection = !self.selected_names().is_empty();
        let names: Vec<String> = self.groups.iter().map(|(name, _)| name.clone()).collect();
        let show = crate::ui::dropdown_list::pick_list(Some(shown), shown_options, |v| v.to_string())
            .on_select(move |picked| {
                Message::LayerGroupShow((picked != all).then_some(picked))
            })
            .text_size(FONT_SZ)
            .padding([3, 6])
            .width(Length::Fixed(170.0));
        let add_to: Element<'_, Message> = if has_selection && !names.is_empty() {
            crate::ui::dropdown_list::pick_list(None::<String>, names, |v| v.to_string())
                .on_select(Message::LayerGroupAdd)
                .placeholder(crate::tr!("layer-groups", "add-to"))
                .text_size(FONT_SZ)
                .padding([3, 6])
                .width(Length::Fixed(150.0))
                .into()
        } else {
            iced::widget::Space::new().width(0).into()
        };
        let in_group = self.group.is_some();
        container(
            row![
                show,
                toolbar_btn_cond(
                    crate::ui::icons::FOLDER_OPEN,
                    crate::tr!("layer-groups", "new-group").into(),
                    Message::LayerGroupNew,
                    has_selection,
                ),
                add_to,
                toolbar_btn_cond(
                    crate::ui::icons::MINUS,
                    crate::tr!("layer-groups", "remove").into(),
                    Message::LayerGroupRemove,
                    in_group && has_selection,
                ),
                toolbar_btn_cond(
                    crate::ui::icons::TRASH,
                    crate::tr!("layer-groups", "delete").into(),
                    Message::LayerGroupDelete,
                    in_group,
                ),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .width(Length::Fill)
        .padding([2, 8])
        .into()
    }

    /// Sync layers + update per-viewport freeze columns.
    /// `vp_info`: list of (vp_handle, vp_label, frozen_layer_handles) from scene.
    pub fn sync_with_viewports(
        &mut self,
        doc_layers: &Table<DocLayer>,
        vp_info: Vec<ViewportLayers>,
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
            .map(|vp| VpCol {
                handle: vp.handle,
                label: vp.label.clone(),
            })
            .collect();

        self.layers = doc_layers
            .iter()
            // The reference's hidden system layers (`*ADSK_CONSTRAINTS`) stay out.
            .filter(|l| !l.name.starts_with('*'))
            .map(|l| {
                let layer_handle = l.handle;
                let vp_frozen = vp_info
                    .iter()
                    .map(|vp| vp.frozen.contains(&layer_handle))
                    .collect();
                let vp_colors = vp_info
                    .iter()
                    .map(|vp| {
                        vp.colors
                            .iter()
                            .find(|(layer, _)| *layer == layer_handle)
                            .map(|(_, color)| *color)
                    })
                    .collect();
                Layer {
                    name: l.name.clone(),
                    visible: !l.flags.off,
                    frozen: l.flags.frozen,
                    locked: l.flags.locked,
                    plottable: l.is_plottable,
                    color: l.color,
                    color_name: l.color_name.clone(),
                    book_name: l.book_name.clone(),
                    linetype: if l.line_type.is_empty() {
                        "Continuous".to_string()
                    } else {
                        l.line_type.clone()
                    },
                    lineweight: l.line_weight,
                    transparency: (l.transparency.as_percent() * 100.0).round() as i32,
                    vp_frozen,
                    vp_colors,
                }
            })
            .collect();

        // Re-resolve the selection against the rebuilt rows by name.
        self.selected = anchor_name.and_then(|n| self.layers.iter().position(|l| l.name == n));
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

    pub fn refresh_sort(&mut self) {
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
                LayerSortCol::Plot => a.plottable.cmp(&b.plottable),
                LayerSortCol::Color => color_sort_key(a.color).cmp(&color_sort_key(b.color)),
                LayerSortCol::Linetype => a.linetype.to_lowercase().cmp(&b.linetype.to_lowercase()),
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

    /// Row indices passing the live name filter — the rows the view actually
    /// draws. Index math over "what the user sees" (shift-ranges) must use
    /// these; raw `selected_multi` ranges would sweep in hidden rows (#50).
    pub fn visible_indices(&self) -> Vec<usize> {
        let filter = self.filter.to_lowercase();
        let members = self.group.as_ref().and_then(|shown| {
            self.groups
                .iter()
                .find(|(name, _)| name == shown)
                .map(|(_, layers)| layers)
        });
        self.layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| members.is_none_or(|members| members.contains(&layer.name)))
            .filter(|(_, layer)| filter.is_empty() || layer.name.to_lowercase().contains(&filter))
            .map(|(i, _)| i)
            .collect()
    }

    /// The Layer Manager as a dock pallet: its title bar, the toolbar across
    /// the pallet (search box on the right), then the table, at least as wide
    /// as the pallet and scrolling sideways when the pallet is narrower.
    /// `active_vp`: the layout viewport being worked in, whose columns are
    /// then the only ones shown.
    pub fn view_panel(
        &self,
        name_col_w: f32,
        width: f32,
        chrome: crate::ui::dock::Chrome,
        active_vp: Option<Handle>,
    ) -> Element<'_, Message> {
        use crate::ui::dock::PanelId;
        let title_bar = crate::ui::dock::title_bar(
            PanelId::Layers,
            crate::tr!("modal", "layer-manager"),
            chrome,
        );
        // Inside the pallet's 1 px border on either side.
        let body = self.view_content(name_col_w, (width - 2.0).max(0.0), active_vp);
        container(column![title_bar, body])
            .width(Length::Fixed(width))
            .height(Length::Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.base.color)),
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    /// Toolbar and table for a pallet `inner_w` wide.
    fn view_content(
        &self,
        name_col_w: f32,
        inner_w: f32,
        active_vp: Option<Handle>,
    ) -> Element<'_, Message> {
        // A viewport the columns do not list yet shows them all.
        let active_vp = active_vp.filter(|h| self.vp_cols.iter().any(|vp| vp.handle == *h));
        let has_sel = self.selected.is_some();
        let sel_is_zero = self
            .selected
            .map(|i| self.layers.get(i).map(|l| l.name == "0").unwrap_or(false))
            .unwrap_or(false);
        let can_set_current = self
            .selected
            .and_then(|i| self.layers.get(i))
            .is_some_and(|layer| layer.name != self.current_layer);

        // ── Toolbar ───────────────────────────────────────────────────────
        let toolbar = container(
            row![
                toolbar_btn(crate::ui::icons::PLUS, t!("New"), Message::LayerNew),
                toolbar_btn_cond(
                    crate::ui::icons::TRASH,
                    t!("Delete"),
                    Message::LayerDelete,
                    has_sel && !sel_is_zero,
                ),
                toolbar_btn_cond(
                    crate::ui::icons::CHECK,
                    t!("Set Current"),
                    Message::LayerSetCurrent,
                    can_set_current,
                ),
                iced::widget::Space::new().width(Length::Fill),
                // Search box: filters rows by name as the user types (#343).
                text_input(t!("Search…").as_ref(), &self.filter)
                    .on_input(Message::LayerManagerFilterChanged)
                    .size(FONT_SZ)
                    .padding([3, 6])
                    .width(Length::Fixed(180.0))
                    .style(|theme, status| field_style_with_radius(theme, status, 2.0)),
            ]
            .spacing(2)
            .align_y(iced::Center),
        )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        })
        .width(Length::Fill)
        .padding([4, 8]);

        // ── Column header ─────────────────────────────────────────────────
        let sc = self.sort_col;
        let sa = self.sort_asc;
        let mut header_row = row![
            text(t!("Status")).size(10).style(muted_style).width(50),
            sortable_header(
                t!("Name"),
                LayerSortCol::Name,
                Length::Fixed(name_col_w),
                sc,
                sa
            ),
            // Draggable divider: adjusts the Name column width (#359).
            iced::widget::mouse_area(
                container(iced::widget::Space::new().width(2).height(14)).style(|theme: &Theme| {
                    container::Style {
                        background: Some(Background::Color(
                            theme.palette().background.neutral.color,
                        )),
                        ..Default::default()
                    }
                },),
            )
            .on_press(Message::LayerNameColGrab)
            .interaction(iced::mouse::Interaction::ResizingHorizontally),
        ]
        .spacing(4)
        .align_y(iced::Center);
        for (col, shown) in column_layout() {
            if shown {
                header_row = header_row.push(sortable_header(
                    col.label(),
                    col,
                    Length::Fixed(col.width()),
                    sc,
                    sa,
                ));
            }
        }

        for vp in self.vp_cols.iter().filter(|vp| active_vp.is_none_or(|h| h == vp.handle)) {
            header_row = header_row.push(
                text(vp.label.as_str())
                    .size(10)
                    .style(muted_style)
                    .width(Length::Fixed(COL_ICON * 2.0 + 4.0)),
            );
        }

        let col_header = container(header_row)
            .style(|theme: &Theme| {
                let palette = theme.palette();
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
            .width(Length::Fill);
        // A right-click on the header chooses the columns.
        let col_header = iced::widget::mouse_area(col_header)
            .on_right_press(Message::LayerColumnsMenu(true));

        // ── Layer rows ────────────────────────────────────────────────────
        let mut rows_col = column![].spacing(0);
        for i in self.visible_indices() {
            let layer = &self.layers[i];
            // Highlight every selected row; show the editable combos only on the
            // anchor (a single shared combo state can't drive several rows).
            let is_anchor = self.selected == Some(i);
            let is_sel = is_anchor || self.selected_multi.contains(&i);
            let is_current = layer.name == self.current_layer;
            let is_editing = self.editing.as_deref() == Some(layer.name.as_str());
            let color_open = self.color_picker_row.as_deref() == Some(layer.name.as_str());

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
                active_vp,
                name_col_w,
            ));
        }

        let table = scrollable(rows_col)
            .id(iced::advanced::widget::Id::new(LAYER_TABLE_SCROLL_ID))
            .width(Length::Fill)
            .height(Length::Fill);

        // The header and table stretch to the wider of their columns and the
        // pallet (the zero-height spacer), so the header's grey band and
        // borders always run to the pallet's edge; a narrower pallet scrolls
        // the table sideways while the toolbar stays put.
        let grid = column![
            col_header,
            table,
            iced::widget::Space::new()
                .width(Length::Fixed(inner_w))
                .height(0),
        ]
        .height(Length::Fill);
        let grid = scrollable(grid)
            .direction(iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Scrollbar::new(),
            ))
            .width(Length::Fill)
            .height(Length::Fill);
        if self.columns_menu {
            column![toolbar, self.group_bar(), column_chooser(), grid].height(Length::Fill).into()
        } else {
            column![toolbar, self.group_bar(), grid].height(Length::Fill).into()
        }
    }
}

/// Every movable column with a tick to show it and arrows to move it; it
/// stays open while several are changed.
fn column_chooser<'a>() -> Element<'a, Message> {
    let mut list = column![].spacing(2);
    let layout = column_layout();
    let last = layout.len().saturating_sub(1);
    for (n, (col, shown)) in layout.into_iter().enumerate() {
        let arrow = |up: bool| {
            let icon = if up {
                crate::ui::icons::themed_arrow_up(9.0)
            } else {
                crate::ui::icons::themed_arrow_down(9.0)
            };
            let enabled = if up { n > 0 } else { n < last };
            button(icon)
                .on_press_maybe(enabled.then_some(Message::LayerColumnMove(col, if up { -1 } else { 1 })))
                .style(button::text)
                .padding([2, 6])
        };
        list = list.push(
            row![
                checkbox(shown)
                    .label(col.label())
                    .text_size(FONT_SZ)
                    .size(13)
                    .on_toggle(move |_| Message::LayerColumnToggle(col))
                    .width(Length::Fill),
                arrow(true),
                arrow(false),
            ]
            .align_y(iced::Center),
        );
    }
    let close = button(text(t!("Close")).size(FONT_SZ))
        .on_press(Message::LayerColumnsMenu(false))
        .padding([2, 10])
        .style(button::secondary);
    container(column![list, close].spacing(6))
        .style(container::bordered_box)
        .padding(6)
        .width(Length::Fixed(220.0))
        .into()
}

// ── Sorting helpers ─────────────────────────────────────────────────────────

/// Packed RGB key for ordering colours deterministically by hue-ish bytes.
fn color_sort_key(c: AcadColor) -> u32 {
    let c = iced_color_from_acad(&c);
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (q(c.r) << 16) | (q(c.g) << 8) | q(c.b)
}

/// A clickable column header that sorts the table by `col`. Shows an up/down
/// SVG arrow when it is the active sort column (#133).
fn sortable_header<'a>(
    label: Cow<'static, str>,
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
        .style(header_button_style)
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

fn toolbar_btn<'a>(
    icon: &'static [u8],
    label: Cow<'static, str>,
    msg: Message,
) -> Element<'a, Message> {
    button(
        row![crate::ui::icons::themed(icon, 12.0), text(label).size(11),]
            .spacing(5)
            .align_y(iced::Center),
    )
    .on_press(msg)
    .style(|theme: &Theme, status| {
        let palette = theme.palette();
        let pair = match status {
            button::Status::Hovered | button::Status::Pressed => palette.background.strong,
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
    label: Cow<'static, str>,
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
                text(label)
                    .size(11)
                    .style(|theme: &Theme| iced::widget::text::Style {
                        color: Some(theme.palette().background.base.text.scale_alpha(0.42)),
                    })
            },
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .style(move |theme: &Theme, status| {
        let palette = theme.palette();
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
            let palette = theme.palette();
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

/// The layer-name field while a rename is open; a click that moves focus
/// elsewhere commits the typed name.
pub fn rename_input_id() -> iced::widget::Id {
    iced::widget::Id::new("layer-rename-input")
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
    active_vp: Option<Handle>,
    name_col_w: f32,
) -> Element<'a, Message> {
    // #22: messages carry the layer name (resolved at dispatch time) so a
    // resort between view() and the handler cannot mistarget the row.
    let lname = layer.name.clone();
    let svg_btn = |bytes: &'static [u8], on_press: Message| -> Element<'a, Message> {
        button(crate::ui::icons::semantic(bytes, ICON_SZ))
            .on_press(on_press)
            .style(cell_button_style(is_selected, index))
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
    let plot_icon: Element<'_, Message> = if layer.plottable {
        crate::ui::icons::themed(crate::ui::icons::PRINT, ICON_SZ)
    } else {
        row![
            crate::ui::icons::themed(crate::ui::icons::PRINT, ICON_SZ),
            crate::ui::icons::themed_danger(crate::ui::icons::CLOSE, ICON_SZ * 0.65),
        ]
        .spacing(0)
        .align_y(iced::Center)
        .into()
    };

    let plot_btn: Element<'_, Message> = button(plot_icon)
        .on_press(Message::LayerTogglePlot(lname.clone()))
        .style(cell_button_style(is_selected, index))
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 4.0,
            right: 4.0,
        })
        .height(Length::Fixed(ROW_H))
        .into();

    let status_dot: Element<'_, Message> = if is_current {
        crate::ui::icons::themed_success(crate::ui::icons::CHECK, 13.0)
    } else {
        crate::ui::icons::themed_secondary(crate::ui::icons::DOT, 9.0)
    };

    // Name cell
    let name_cell: Element<'_, Message> = if is_editing {
        text_input("", edit_buf)
            .id(rename_input_id())
            .on_input(Message::LayerRenameEdit)
            .on_submit(Message::LayerRenameCommit)
            .size(FONT_SZ)
            .padding(Padding {
                top: COMBO_PAD_V,
                bottom: COMBO_PAD_V,
                left: 4.0,
                right: 4.0,
            })
            .style(|theme, status| field_style_with_radius(theme, status, 2.0))
            .width(Length::Fixed(name_col_w))
            .into()
    } else {
        // ~6 px per glyph at the 10 px row font — track the column width.
        let name_budget = ((name_col_w / 6.0) as usize).max(8);
        // One line, cut at the cell: the glyph estimate can run over a narrow
        // column, and a wrapped name spilled into the rows below (#2251).
        let name_btn = button(
            text(crate::ui::text_util::elide(&layer.name, name_budget))
                .size(FONT_SZ)
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .clip(true)
        .on_press(Message::LayerRenameStart(lname.clone()))
        .style(cell_button_style(is_selected, index))
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
            tooltip(
                name_btn,
                name_tip(&layer.name),
                tooltip::Position::FollowCursor,
            )
            .into()
        } else {
            name_btn.into()
        }
    };

    // Color cell — looks like a combo_box input; click opens swatch dropdown below row.
    // Shared colour selector. Layers carry a concrete colour (no ByLayer /
    // ByBlock); true colours stay RGB instead of being collapsed to ACI 7.
    let color_cell: Element<'_, Message> =
        container(crate::ui::color_select::color_selector_with_name(
            layer.color,
            layer.color_name.as_deref(),
            color_picker_open,
            crate::ui::color_select::ColorExtras {
                by_layer: false,
                by_block: false,
                ..Default::default()
            },
            Message::LayerColorSet,
            Message::LayerColorPickerToggle(lname.clone()),
            Message::OpenColorWindow(
                crate::app::ColorPickTarget::Layer(lname.clone()),
                layer.color,
            ),
        ))
        .width(Length::Fixed(COL_COLOR))
        .into();

    // Linetype cell — uses LinetypeItem (with ASCII art) same as Properties panel
    let cur_lt_item = LinetypeItem {
        name: layer.linetype.clone(),
        art: String::new(), // art comes from combo state items; just match by name
    };
    let lt_cell: Element<'_, Message> = if let Some(state) = lt_combo {
        crate::ui::wide_menu::wide_menu(
            combo_box(
                state,
                t!("linetype").as_ref(),
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
            .input_style(|theme, status| field_style_with_radius(theme, status, 2.0)),
            LINETYPE_MENU_W,
        )
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
        combo_box(
            state,
            t!("lineweight").as_ref(),
            Some(&cur_lw_item),
            |item: LwItem| Message::LayerLineweightSet(item.0),
        )
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 4.0,
            right: 4.0,
        })
        .width(Length::Fixed(COL_LW))
        .input_style(|theme, status| field_style_with_radius(theme, status, 2.0))
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
    let lname_trans = lname.clone();
    let trans_cell = text_input("0", &trans_str)
        .on_input(move |s| Message::LayerTransparencyEdit(lname_trans.clone(), s))
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 4.0,
            right: 4.0,
        })
        .style(|theme: &Theme, status| {
            let mut style = field_style_with_radius(theme, status, 2.0);
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
    ]
    .spacing(4)
    .width(Fill)
    .align_y(iced::Center);
    // The movable columns, in the chosen order.
    let mut cells: Vec<(LayerSortCol, Element<'a, Message>)> = vec![
        (
            LayerSortCol::On,
            container(svg_btn(vis_svg, Message::LayerToggleVisible(lname.clone())))
                .width(Length::Fixed(COL_ICON))
                .align_x(iced::Center)
                .into(),
        ),
        (
            LayerSortCol::Freeze,
            container(svg_btn(frz_svg, Message::LayerToggleFreeze(lname.clone())))
                .width(Length::Fixed(COL_ICON))
                .align_x(iced::Center)
                .into(),
        ),
        (
            LayerSortCol::Lock,
            container(svg_btn(lck_svg, Message::LayerToggleLock(lname.clone())))
                .width(Length::Fixed(COL_ICON))
                .align_x(iced::Center)
                .into(),
        ),
        (
            LayerSortCol::Plot,
            container(plot_btn)
                .width(Length::Fixed(COL_ICON))
                .align_x(iced::Center)
                .into(),
        ),
        (LayerSortCol::Color, color_cell),
        (LayerSortCol::Linetype, lt_cell),
        (LayerSortCol::Lineweight, lw_cell),
        (LayerSortCol::Transparency, trans_cell.into()),
    ];
    for (col, shown) in column_layout() {
        if let Some(at) = cells.iter().position(|(c, _)| *c == col) {
            let (_, cell) = cells.swap_remove(at);
            if shown {
                row_content = row_content.push(cell);
            }
        }
    }

    // Per-viewport freeze columns
    for (vp_idx, vp_col) in vp_cols.iter().enumerate() {
        if active_vp.is_some_and(|h| h != vp_col.handle) {
            continue;
        }
        let is_vp_frozen = layer.vp_frozen.get(vp_idx).copied().unwrap_or(false);
        let vp_frz_svg = crate::ui::icons::layer_vp_freeze(is_vp_frozen);
        // The viewport's colour for this layer: its override, else the layer's.
        let vp_color = layer.vp_colors.get(vp_idx).copied().flatten();
        let shown = vp_color.unwrap_or(layer.color);
        let vp_color_btn = button(crate::ui::color_select::swatch(iced_color_from_acad(&shown)))
            .on_press(Message::OpenColorWindow(
                crate::app::ColorPickTarget::LayerViewport(lname.clone(), vp_col.handle),
                shown,
            ))
            .padding(2)
            .style(button::text);
        row_content = row_content.push(
            row![
                container(svg_btn(
                    vp_frz_svg,
                    Message::LayerToggleVpFreeze(lname.clone(), vp_idx),
                ))
                .width(Length::Fixed(COL_ICON))
                .align_x(iced::Center),
                container(vp_color_btn)
                    .width(Length::Fixed(COL_ICON))
                    .align_x(iced::Center),
            ]
            .spacing(4),
        );
    }

    mouse_area(
        container(row_content)
            .style(move |theme: &Theme| {
                let palette = theme.palette();
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
    .on_press(Message::LayerSelect(lname))
    .into()
}

// ── Display helpers ───────────────────────────────────────────────────────

pub fn iced_color_from_acad(c: &AcadColor) -> Color {
    match c {
        AcadColor::Index(_) | AcadColor::Rgb { .. } => {
            crate::ui::properties::acad_color_display(*c).0
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

#[cfg(test)]
mod column_layout_tests {
    use super::*;

    /// #2180: columns hide, move, and come back from their saved names.
    #[test]
    fn columns_hide_move_and_round_trip() {
        set_column_layout_names(&[]);
        toggle_column(LayerSortCol::Plot);
        move_column(LayerSortCol::Color, -1);
        move_column(LayerSortCol::On, -1);
        let names = column_layout_names();
        assert_eq!(names[..5], ["On", "Freeze", "Lock", "Color", "-Plot"]);
        set_column_layout_names(&["-Lock".into(), "Transparency".into(), "bogus".into()]);
        let layout = column_layout();
        assert_eq!(layout[0], (LayerSortCol::Lock, false));
        assert_eq!(layout[1], (LayerSortCol::Transparency, true));
        assert_eq!(layout.len(), COLUMNS.len());
    }
}
