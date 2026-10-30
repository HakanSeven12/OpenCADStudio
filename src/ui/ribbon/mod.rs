// Ribbon — tab bar + 3-row tool area.
//
// Button sizes:
//   LargeTool / LargeDropdown  — full ribbon height (3 rows), icon + label [+ ▾]
//   Tool / Dropdown            — 1-row height, icon only [+ ▾ on right]
//
// Dropdown items within a group are collected into columns of 3 rows.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use rustc_hash::FxHashMap as HashMap;

use codec::types::{Color as AcadColor, LineWeight};
use iced::widget::{button, column, container, mouse_area, row, scrollable, text, Space};
use iced::{Background, Border, Color, Element, Fill, Length, Padding, Theme};

use crate::app::Message;
use crate::modules::{CadModule, IconKind, RibbonGroup, RibbonItem, ToolDef};
use crate::plugin::all_ribbon_modules;
use crate::ui::properties::{LinetypeItem, linetype_display_name, lw_options};

mod draw_panel;
pub(crate) mod extras;
pub(crate) mod widgets;
pub(crate) use draw_panel::tools as panel_tools;
mod color_dropdown;
mod context_tools;
mod visibility_panel;
pub use context_tools::{UnderlayContext, pdf_underlay_tools, point_cloud_tools, xref_tools};
pub(crate) use widgets::{REDO_HISTORY_ID, UNDO_HISTORY_ID};
use widgets::{StyleContext, *};
mod app_menu;
mod collapse;
mod keytips;
mod layout;
mod locate;
mod minimize;
mod panel_frame;
mod ribbon_menu;
mod search;
mod stretch_column;
use crate::t;
use crate::ui::style::{chrome, ribbon_metrics as metrics};
use crate::ui::wrap_bar::{PosReport, WrapBar, WrapFlow};
pub use app_menu::{works_without_drawing, APP_MENU_ID};
pub use collapse::CollapseMode;
use collapse::{CollapsePanels, Panel};
pub use draw_panel::SlideOutMsg;
pub use keytips::{
    KeyTipInput, KeyTipStep, active as keytips_active, key_pressed as keytip_key_pressed,
    key_released as keytip_key_released, pointer_pressed as keytip_pointer_pressed,
};
pub use layout::{MinimizePref, RibbonLayout, RibbonMinimize};
pub use minimize::{MinimizeMsg, Opener};
pub use ribbon_menu::{RibbonMenuMsg, Submenu};
pub use search::{results_scroll_id, SearchMsg, SearchOutcome, SEARCH_INPUT_ID};

pub(crate) fn tooltip_content(text: String) -> Element<'static, Message> {
    widgets::make_tip(text)
}

pub(crate) fn tooltip_style(theme: &Theme) -> container::Style {
    widgets::tip_style(theme)
}

/// A line over the items of a split button whose face names the chosen item.
fn dropdown_header(id: &str) -> Option<&'static str> {
    (id == "IMPORT_FILE").then_some("Import File — the face runs the last one chosen")
}

fn hash_tool_layout(tool: &ToolDef, hasher: &mut impl Hasher) {
    tool.id.hash(hasher);
    t!(tool.label).as_ref().hash(hasher);
}

fn hash_item_layout(item: &RibbonItem, hasher: &mut impl Hasher) {
    match item {
        RibbonItem::Tool(tool) => {
            0u8.hash(hasher);
            hash_tool_layout(tool, hasher);
        }
        RibbonItem::LabeledTool(tool) => {
            1u8.hash(hasher);
            hash_tool_layout(tool, hasher);
        }
        RibbonItem::LargeTool(tool) => {
            2u8.hash(hasher);
            hash_tool_layout(tool, hasher);
        }
        RibbonItem::Dropdown { id, items, .. } => {
            3u8.hash(hasher);
            id.hash(hasher);
            items.len().hash(hasher);
            for (command, label, _) in items {
                command.hash(hasher);
                t!(*label).as_ref().hash(hasher);
            }
        }
        RibbonItem::LabeledDropdown {
            id, label, items, ..
        } => {
            4u8.hash(hasher);
            id.hash(hasher);
            t!(*label).as_ref().hash(hasher);
            items.len().hash(hasher);
            for (command, item_label, _) in items {
                command.hash(hasher);
                t!(*item_label).as_ref().hash(hasher);
            }
        }
        RibbonItem::LargeDropdown {
            id, label, items, ..
        } => {
            5u8.hash(hasher);
            id.hash(hasher);
            t!(*label).as_ref().hash(hasher);
            items.len().hash(hasher);
            for (command, item_label, _) in items {
                command.hash(hasher);
                t!(*item_label).as_ref().hash(hasher);
            }
        }
        RibbonItem::ToolGrid { columns } => {
            6u8.hash(hasher);
            columns.len().hash(hasher);
            for column in columns {
                column.len().hash(hasher);
                for tool in column {
                    hash_tool_layout(tool, hasher);
                }
            }
        }
        RibbonItem::LayerComboGroup { row2, row3 } => {
            7u8.hash(hasher);
            row2.len().hash(hasher);
            row3.len().hash(hasher);
            for tool in row2.iter().chain(row3) {
                hash_tool_layout(tool, hasher);
            }
        }
        RibbonItem::PropertiesGroup { match_prop } => {
            8u8.hash(hasher);
            hash_tool_layout(match_prop, hasher);
        }
        RibbonItem::StyleComboGroup {
            style_key,
            combo_id,
            rows,
            ..
        } => {
            9u8.hash(hasher);
            format!("{style_key:?}").hash(hasher);
            combo_id.hash(hasher);
            for row in rows {
                row.len().hash(hasher);
                for tool in row {
                    hash_tool_layout(tool, hasher);
                }
            }
        }
    }
}

fn panel_measure_key(
    groups: &[&RibbonGroup],
    last_cmd: &HashMap<&'static str, &'static str>,
    style_ctx: &StyleContext<'_>,
    visibility: Option<&crate::ui::window::block_visibility::RibbonContext>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    crate::sys::ui_font::generation().hash(&mut hasher);
    groups.len().hash(&mut hasher);
    for group in groups {
        group.title.hash(&mut hasher);
        group.tools.len().hash(&mut hasher);
        for item in &group.tools {
            hash_item_layout(item, &mut hasher);
        }
    }
    let mut commands: Vec<_> = last_cmd.iter().collect();
    commands.sort_unstable_by_key(|(id, _)| **id);
    for (id, command) in commands {
        id.hash(&mut hasher);
        command.hash(&mut hasher);
    }
    for (names, active) in [
        (style_ctx.text_style_names, style_ctx.active_text_style),
        (style_ctx.dim_style_names, style_ctx.active_dim_style),
        (
            style_ctx.mleader_style_names,
            style_ctx.active_mleader_style,
        ),
        (style_ctx.table_style_names, style_ctx.active_table_style),
    ] {
        names.hash(&mut hasher);
        active.hash(&mut hasher);
    }
    format!("{visibility:?}").hash(&mut hasher);
    hasher.finish()
}

/// Height of a tab, and of the tab row it stands at the bottom of.
const TAB_H: f32 = metrics::TAB_H;
const TAB_ROW_H: f32 = metrics::TAB_ROW_H;
const TAB_TEXT_SIZE: f32 = 12.5;
/// Width of the accent outline of a contextual tab.
const TAB_OUTLINE: f32 = 1.5;

/// `tab` with an accent line along its top and sides.
fn tab_outline(tab: Element<'_, Message>) -> Element<'_, Message> {
    let line = |width: Length, height: Length| {
        container(iced::widget::Space::new().width(width).height(height)).style(|theme: &Theme| {
            container::Style {
                background: Some(Background::Color(chrome::accent(theme))),
                ..Default::default()
            }
        })
    };
    iced::widget::stack![
        tab,
        container(line(Fill, Length::Fixed(TAB_OUTLINE)))
            .width(Fill)
            .height(Fill)
            .align_top(Fill),
        container(line(Length::Fixed(TAB_OUTLINE), Fill))
            .width(Fill)
            .height(Fill)
            .align_left(Fill),
        container(line(Length::Fixed(TAB_OUTLINE), Fill))
            .width(Fill)
            .height(Fill)
            .align_right(Fill),
    ]
    .into()
}

/// The height of the tool area, with or without panel titles.
fn panel_height(titles: bool) -> f32 {
    if titles {
        TOOL_BAR_H
    } else {
        TOOL_BAR_H_UNTITLED
    }
}

/// The width of a contextual tab's outline and of the line along the top of
/// its tool area. The design's 1.5 px draws as one faint pixel on a 1x
/// screen; 2 px stays crisp.
const CONTEXTUAL_LINE: f32 = 2.0;

/// Whether module `id`'s tab is contextual: shown only while its context
/// lasts (a block being edited).
fn is_contextual(id: &str) -> bool {
    id == "block_editor"
}

/// The quick-access file buttons before Undo and Redo: icon, label and
/// command.
static QUICK_ACCESS: [(&[u8], &str, &str); 5] = [
    (crate::ui::icons::DOC_NEW, "New", "QNEW"),
    (crate::ui::icons::FOLDER_OPEN, "Open", "OPEN"),
    (crate::ui::icons::SAVE, "Save", "SAVE"),
    (crate::ui::icons::FILE_EXPORT, "Save As", "SAVEAS"),
    (crate::ui::icons::PRINT, "Print", "PRINT"),
];

// ── Ribbon state ───────────────────────────────────────────────────────────

pub struct Ribbon {
    modules: Vec<Box<dyn CadModule>>,
    active: usize,
    active_tool: Option<String>,
    pub wireframe: bool,
    pub ortho_mode: bool,
    /// ViewCube (NAVVCUBE) visibility — drives the View Cube button highlight.
    pub show_viewcube: bool,
    /// UCS icon (UCSICON) visibility — drives the UCS Icon button highlight.
    pub show_ucs_icon: bool,
    /// Properties panel (PROPERTIES) visibility — drives the Properties button highlight.
    pub show_properties: bool,
    /// The Count palette is open (View › Palettes › Count).
    pub show_count_palette: bool,
    /// Sheet Set Manager palette shown (SSMSTATE).
    pub show_sheet_set: bool,
    /// Graphic Attributes panel visibility — drives its ribbon button highlight.
    pub show_graphic_attributes: bool,
    pub show_authoring_palettes: bool,
    /// File tabs (FILETAB) visibility — drives the File Tabs button highlight.
    pub show_file_tabs: bool,
    /// Layout tabs (LAYOUTTAB) visibility — drives the Layout Tabs button highlight.
    pub show_layout_tabs: bool,
    pub open_dropdown: Option<String>,
    /// Title of the collapsed or minimized panel whose peek is open, if any.
    pub collapsed_open: Option<String>,
    last_cmd: HashMap<&'static str, &'static str>,
    pub active_layer: String,
    pub layer_infos: Vec<LayerInfo>,
    /// Live filter for the layer dropdown's search box (#343). Cleared on
    /// close so the next open starts unfiltered.
    pub layer_filter: String,
    /// Active object color — ACI / ByLayer / ByBlock.
    pub active_color: AcadColor,
    /// Colour-book identity (`BOOK$NAME`) of a colour picked from a colour
    /// book, with the colour it was picked as; see `active_color_name`.
    pub active_color_book: Option<(AcadColor, String)>,
    /// Active linetype override ("ByLayer", "Continuous", …).
    pub active_linetype: String,
    /// Active lineweight.
    pub active_lineweight: LineWeight,
    /// Linetypes loaded from the current document (with ASCII art).
    pub available_linetypes: Vec<LinetypeItem>,
    /// Whether the full ACI palette is expanded inside the color picker overlay.
    pub prop_color_palette_open: bool,
    // ── Style selector state ──────────────────────────────────────────────
    pub text_style_names: Vec<String>,
    pub active_text_style: String,
    pub dim_style_names: Vec<String>,
    pub active_dim_style: String,
    pub mleader_style_names: Vec<String>,
    pub active_mleader_style: String,
    pub table_style_names: Vec<String>,
    pub active_table_style: String,
    /// Measured tab-bar height (28 on one row, 56 when tabs wrap). Written by
    /// the `WrapBar` widget during layout, read when anchoring dropdowns.
    tab_bar_h: Arc<AtomicU32>,
    /// Measured tool-area height (TOOL_BAR_H at full density, shorter once every
    /// panel is collapsed). Written by `CollapsePanels`, read when anchoring
    /// dropdowns below the ribbon.
    tool_bar_h: Arc<AtomicU32>,
    /// Panel density: `Auto` sizes the panels to the window; tests pin one.
    collapse_mode: CollapseMode,
    /// Hidden tabs and panels, panel order and the like (persisted).
    layout: RibbonLayout,
    /// The right-click menu, while open.
    menu: Option<ribbon_menu::MenuState>,
    /// Popups of the minimized ribbon.
    min: minimize::MinState,
    /// Where the pointer is, for closing a slide-out.
    slide: draw_panel::SlideState,
    /// The command search.
    search: search::SearchState,
    /// The window size, for the search box and its results.
    win_w: f32,
    win_h: f32,
    /// Set by `CollapsePanels` when the tool row is tight; the minimize
    /// button hides then to give the cramped tab row its space back.
    collapse_tight: Arc<AtomicBool>,
    /// The selected PDF underlay's switches while only underlays are selected.
    underlay_ctx: Option<UnderlayContext>,
    /// Only xrefs are selected.
    xref_ctx: bool,
    /// Only point clouds are selected: whether the first shows its crops.
    point_cloud_ctx: Option<bool>,
    /// A block editor session is open: the tab that was active before the
    /// Block Editor tab took over.
    block_editing: Option<usize>,
    block_testing: Option<usize>,
    /// XDWGFADECTL as the Reference slide-out shows it (negative = off).
    pub xref_fade: i32,
    /// Open key tips (Alt).
    keytips: Option<keytips::State>,
}

/// Per-layer display data shown in the ribbon layer dropdown.
#[derive(Clone, Debug)]
pub struct LayerInfo {
    pub name: String,
    pub color: Color,
    pub visible: bool,
    pub frozen: bool,
    pub locked: bool,
    /// Layout with an active viewport: (its Layer Manager column, frozen there).
    pub vp_frozen: Option<(usize, bool)>,
}

/// The colour of layer `name` in the ribbon's layer list.
pub(super) fn layer_color(layers: &[LayerInfo], name: &str) -> Option<Color> {
    layers
        .iter()
        .find(|layer| layer.name == name)
        .map(|layer| layer.color)
}

/// Full-screen backdrop for an open ribbon dropdown: closes the dropdown when
/// the user clicks outside the panel. (Cursor motion leaking to the viewport
/// beneath is blocked in `on_viewport_move`, not here — iced 0.14's mouse_area
/// can only capture button presses, never CursorMoved. #227.)
fn dropdown_backdrop<'a>(positioned: Element<'a, Message>) -> Element<'a, Message> {
    mouse_area(positioned)
        .on_press(Message::CloseRibbonDropdown)
        .into()
}

/// Position a ribbon dropdown `panel` in a full-window container just below its
/// widget, growing left or right per `align_right` (see [`Ribbon::dd_anchor`]).
fn position_ribbon_dropdown<'a>(
    panel: Element<'a, Message>,
    align_right: bool,
    h_pad: f32,
    top: f32,
) -> Element<'a, Message> {
    let pad = Padding {
        top,
        bottom: 0.0,
        left: if align_right { 0.0 } else { h_pad },
        right: if align_right { h_pad } else { 0.0 },
    };
    let c = container(panel)
        .align_top(Fill)
        .width(Fill)
        .height(Fill)
        .padding(pad);
    let c = if align_right {
        c.align_right(Fill)
    } else {
        c.align_left(Fill)
    };
    c.into()
}

impl Ribbon {
    /// The current layer's colour: what ByLayer looks like for new objects.
    pub fn active_layer_color(&self) -> Option<Color> {
        layer_color(&self.layer_infos, &self.active_layer)
    }

    /// The colour-book identity of the active colour, while the active colour
    /// is still the one picked from the book.
    pub fn active_color_name(&self) -> Option<String> {
        self.active_color_book
            .as_ref()
            .filter(|(color, _)| *color == self.active_color)
            .map(|(_, name)| name.clone())
    }

    pub fn new() -> Self {
        Self {
            modules: all_ribbon_modules(),
            active: 0,
            active_tool: None,
            wireframe: false,
            ortho_mode: true,
            show_viewcube: true,
            show_ucs_icon: true,
            show_properties: true,
            show_count_palette: false,
            show_sheet_set: false,
            show_graphic_attributes: false,
            show_authoring_palettes: false,
            show_file_tabs: true,
            show_layout_tabs: true,
            open_dropdown: None,
            collapsed_open: None,
            last_cmd: HashMap::default(),
            // Empty until a document is open — populated by sync_ribbon_layers.
            active_layer: String::new(),
            layer_infos: vec![],
            layer_filter: String::new(),
            active_color: AcadColor::ByLayer,
            active_color_book: None,
            active_linetype: "ByLayer".to_string(),
            active_lineweight: LineWeight::ByLayer,
            available_linetypes: vec![LinetypeItem {
                name: "Continuous".to_string(),
                art: String::new(),
            }],
            prop_color_palette_open: false,
            // Empty until a document is open — the Annotate style dropdowns
            // are populated from the active document by sync_ribbon_styles.
            text_style_names: vec![],
            active_text_style: String::new(),
            dim_style_names: vec![],
            active_dim_style: String::new(),
            mleader_style_names: vec![],
            active_mleader_style: String::new(),
            table_style_names: vec![],
            active_table_style: String::new(),
            tab_bar_h: Arc::new(AtomicU32::new(TAB_ROW_H.to_bits())),
            tool_bar_h: Arc::new(AtomicU32::new(TOOL_BAR_H.to_bits())),
            collapse_mode: CollapseMode::default(),
            layout: RibbonLayout::default(),
            menu: None,
            min: minimize::MinState::default(),
            slide: draw_panel::SlideState::default(),
            search: search::SearchState::default(),
            win_w: f32::INFINITY,
            win_h: f32::INFINITY,
            collapse_tight: Arc::new(AtomicBool::new(false)),
            underlay_ctx: None,
            xref_ctx: false,
            point_cloud_ctx: None,
            block_editing: None,
            block_testing: None,
            xref_fade: 50,
            keytips: None,
        }
    }

    /// The window size changed.
    pub fn set_window_size(&mut self, (w, h): (f32, f32)) {
        // A wider window may show the peeked panel in full again.
        if w > self.win_w && self.layout.minimize == RibbonMinimize::Off {
            self.close_popup();
        }
        self.win_w = w;
        self.win_h = h;
    }

    /// Pin the tool panels to one density, so a test sees a known layout.
    #[cfg(test)]
    pub fn set_collapse_mode(&mut self, mode: CollapseMode) {
        self.collapse_mode = mode;
    }

    /// Apply a saved layout. Persistence is handled by the caller via the
    /// consolidated app config (`save_config`).
    pub fn set_layout(&mut self, layout: RibbonLayout) {
        self.layout = layout;
        self.show_a_visible_tab();
    }

    /// The current layout (for saving into the app config).
    pub fn layout(&self) -> &RibbonLayout {
        &self.layout
    }

    /// Whether the ribbon shows its default layout: panel order, hidden tabs
    /// and panels, and panel titles.
    pub fn is_default_layout(&self) -> bool {
        self.layout.is_default(&self.modules)
    }

    /// Whether tab `index` is left out of the tab row because the user hid
    /// it. The tab on show and the contextual tabs are never left out.
    fn tab_hidden(&self, index: usize) -> bool {
        index != self.active
            && self.modules.get(index).is_some_and(|module| {
                !matches!(module.id(), "layout" | "block_editor")
                    && self.layout.hidden_tabs.contains(module.id())
            })
    }

    /// Move off a tab the user hid onto the first one still shown.
    fn show_a_visible_tab(&mut self) {
        let hidden = |module: &dyn CadModule| self.layout.hidden_tabs.contains(module.id());
        if !self
            .modules
            .get(self.active)
            .is_some_and(|module| hidden(module.as_ref()))
        {
            return;
        }
        if let Some(index) = self.modules.iter().position(|module| {
            !hidden(module.as_ref()) && !matches!(module.id(), "layout" | "block_editor")
        }) {
            self.active = index;
        }
    }

    /// Current tab-bar height as last measured by the `WrapBar` widget.
    fn tab_bar_height(&self) -> f32 {
        f32::from_bits(self.tab_bar_h.load(Ordering::Relaxed))
    }

    /// Current tool-area height as last measured by the `DensitySwap` widget.
    fn tool_bar_height(&self) -> f32 {
        f32::from_bits(self.tool_bar_h.load(Ordering::Relaxed))
    }

    /// (align_right, horizontal_pad, top) to anchor the open dropdown `id`'s
    /// overlay just below its widget. It grows rightward from the widget's left
    /// edge, but flips to right-aligned (growing left) when a panel of `panel_w`
    /// would cross the right window edge `win_w`. Falls back to below the whole
    /// ribbon when the widget's position hasn't been recorded yet.
    fn dd_anchor(&self, id: &str, panel_w: f32, win_w: f32) -> (bool, f32, f32) {
        match crate::ui::wrap_bar::dropdown_bounds(id) {
            Some(b) => {
                let top = b.y + b.height;
                if b.x + panel_w > win_w - 2.0 {
                    (true, (win_w - (b.x + b.width)).max(4.0), top)
                } else {
                    (false, b.x.max(0.0), top)
                }
            }
            None => (false, 0.0, self.tab_bar_height() + self.tool_bar_height()),
        }
    }

    pub fn set_styles(
        &mut self,
        text: Vec<String>,
        active_text: &str,
        dim: Vec<String>,
        active_dim: &str,
        mleader: Vec<String>,
        active_mleader: &str,
        table: Vec<String>,
        active_table: &str,
    ) {
        self.text_style_names = text;
        self.active_text_style = active_text.to_string();
        self.dim_style_names = dim;
        self.active_dim_style = active_dim.to_string();
        self.mleader_style_names = mleader;
        self.active_mleader_style = active_mleader.to_string();
        self.table_style_names = table;
        self.active_table_style = active_table.to_string();
    }

    pub fn set_layers(&mut self, infos: Vec<LayerInfo>, active: &str) {
        self.active_layer = active.to_string();
        self.layer_infos = infos;
    }

    pub fn set_available_linetypes(&mut self, items: Vec<LinetypeItem>) {
        self.available_linetypes = items;
    }

    /// Show the Block Editor tab while a block is being edited, and return to
    /// the previous tab when the editing ends.
    pub fn set_block_editing(&mut self, editing: bool) {
        let Some(index) = self
            .modules
            .iter()
            .position(|module| module.id() == "block_editor")
        else {
            return;
        };
        match (editing, self.block_editing) {
            (true, None) => {
                self.block_editing = Some(self.active);
                self.active = index;
            }
            (false, Some(previous)) => {
                self.block_editing = None;
                if self.active == index {
                    self.active = previous.min(self.modules.len().saturating_sub(1));
                }
            }
            _ => {}
        }
    }

    pub fn set_block_testing(&mut self, testing: bool) {
        if testing != self.block_testing.is_some() {
            self.close_dropdown();
            let _ = self.hide_keytips();
        }
        let Some(index) = self.modules.iter().position(|module| module.id() == "draw") else {
            return;
        };
        match (testing, self.block_testing) {
            (true, None) => {
                self.block_testing = Some(self.active);
                self.active = index;
            }
            (false, Some(previous)) => {
                self.block_testing = None;
                if self.active == index {
                    self.active = previous.min(self.modules.len().saturating_sub(1));
                }
            }
            _ => {}
        }
    }

    /// The panels of tab `index` in the user's order, hidden ones included.
    /// Contextual panels share their normal tab with the drawing tools.
    fn ordered_groups(&self, index: usize) -> Vec<&RibbonGroup> {
        let testing = self.block_testing.is_some();
        let groups = self.modules.get(index).map(|module| {
            let groups = module
                .ribbon_groups()
                .iter()
                .filter(|group| module.id() != "draw" || group.title != "Close" || testing)
                .collect();
            self.layout.order(module.id(), groups)
        });
        groups.unwrap_or_default()
    }

    /// The panels of tab `index` as the ribbon shows them: in the user's
    /// order, without the hidden ones.
    fn groups_for(&self, index: usize) -> impl Iterator<Item = &RibbonGroup> {
        let module_id = self.modules.get(index).map_or("", |module| module.id());
        let mut groups = self.ordered_groups(index);
        groups.retain(|group| !self.layout.panel_hidden(module_id, group.title));
        groups.into_iter()
    }

    /// Save the panels of module `module_id` in the order a drag left them.
    /// `titles` are the panels shown, in their new order; hidden panels keep
    /// their places.
    pub fn reorder_panels(&mut self, module_id: &str, titles: &[String]) {
        let Some(index) = self.modules.iter().position(|m| m.id() == module_id) else {
            return;
        };
        let mut order: Vec<&'static str> = self
            .ordered_groups(index)
            .iter()
            .map(|group| group.title)
            .collect();
        let slots: Vec<usize> = (0..order.len())
            .filter(|&i| titles.iter().any(|title| title == order[i]))
            .collect();
        let moved: Vec<&'static str> = titles
            .iter()
            .filter_map(|title| order.iter().find(|t| *t == title).copied())
            .collect();
        if moved.len() != slots.len() {
            return;
        }
        for (slot, title) in slots.into_iter().zip(moved) {
            order[slot] = title;
        }
        self.layout.panel_order.insert(
            module_id.to_string(),
            order
                .iter()
                .map(|title| extras::panel_key(module_id, title))
                .collect(),
        );
    }

    /// Height of the tool area at full density: shorter without panel titles.
    fn panel_h(&self) -> f32 {
        panel_height(self.layout.show_panel_titles)
    }

    pub fn select(&mut self, index: usize) {
        if index < self.modules.len() {
            self.active = index;
        }
    }

    /// Replace the tab list (e.g. after a plugin is enabled/disabled in the
    /// Plugin Manager). Clamps the active tab so it stays in range.
    pub fn set_modules(&mut self, modules: Vec<Box<dyn CadModule>>) {
        self.modules = modules;
        if self.active >= self.modules.len() {
            self.active = self.modules.len().saturating_sub(1);
        }
        self.active_tool = None;
        self.open_dropdown = None;
        self.show_a_visible_tab();
    }
    pub fn activate_tool(&mut self, id: &str) {
        self.active_tool = Some(id.to_string());
    }
    #[cfg(test)]
    pub(crate) fn active_tool(&self) -> Option<&str> {
        self.active_tool.as_deref()
    }
    pub fn deactivate_tool(&mut self) {
        self.active_tool = None;
    }
    /// Clear `active_tool` only when it currently equals `id`. Used by the
    /// window-close path to deactivate the tool that owned a popup window
    /// without disturbing a different tool the user picked in the
    /// meantime. See #40.
    pub fn deactivate_tool_if(&mut self, id: &str) {
        if self.active_tool.as_deref() == Some(id) {
            self.active_tool = None;
        }
    }
    pub fn set_wireframe(&mut self, w: bool) {
        self.wireframe = w;
    }
    pub fn set_ortho(&mut self, ortho: bool) {
        self.ortho_mode = ortho;
    }
    pub fn set_viewcube(&mut self, on: bool) {
        self.show_viewcube = on;
    }
    pub fn set_ucs_icon(&mut self, on: bool) {
        self.show_ucs_icon = on;
    }
    pub fn set_properties(&mut self, on: bool) {
        self.show_properties = on;
    }
    pub fn set_count_palette(&mut self, on: bool) {
        self.show_count_palette = on;
    }
    pub fn set_sheet_set(&mut self, on: bool) {
        self.show_sheet_set = on;
    }
    pub fn set_graphic_attributes(&mut self, on: bool) {
        self.show_graphic_attributes = on;
    }
    pub fn set_file_tabs(&mut self, on: bool) {
        self.show_file_tabs = on;
    }
    pub fn set_layout_tabs(&mut self, on: bool) {
        self.show_layout_tabs = on;
    }
    /// Snapshot of every ribbon toggle's live state, for the render path.
    /// The Block Palette highlight is threaded in from the app's authoritative
    /// `show_block_palette` rather than a ribbon copy, so it can't drift from
    /// the panel's true visibility.
    fn toggle_state(&self, show_block_palette: bool) -> widgets::ToggleState {
        use widgets::ToggleState;
        ToggleState {
            ortho_mode: self.ortho_mode,
            show_viewcube: self.show_viewcube,
            show_ucs_icon: self.show_ucs_icon,
            show_properties: self.show_properties,
            show_count_palette: self.show_count_palette,
            show_graphic_attributes: self.show_graphic_attributes,
            show_authoring_palettes: self.show_authoring_palettes,
            show_block_palette,
            show_sheet_set: self.show_sheet_set,
            show_file_tabs: self.show_file_tabs,
            show_layout_tabs: self.show_layout_tabs,
        }
    }

    /// One panel in its four densities; `lead` adds controls before the
    /// group's own tools.
    fn panel<'a>(
        &'a self,
        module_id: &str,
        g: &'a RibbonGroup,
        ts: widgets::ToggleState,
        style_ctx: StyleContext<'a>,
    ) -> Panel<'a> {
        let key = extras::panel_key(module_id, g.title);
        let button_key = key.clone();
        let strip = if self.layout.show_panel_titles {
            Strip::Titled
        } else {
            Strip::Untitled
        };
        let look = self.slide_look(&key);
        let group = move |compact: bool| {
            render_group(
                compact,
                g,
                &key,
                strip,
                look,
                Vec::new(),
                self.lit_tool(),
                &self.open_dropdown,
                &self.last_cmd,
                ts,
                &self.layer_infos,
                &self.active_layer,
                self.active_color,
                &self.active_linetype,
                self.active_lineweight,
                &style_ctx,
            )
        };
        let button = move |tight: bool| {
            // The button lights while one of the panel's tools runs.
            let holds = self
                .lit_tool()
                .as_deref()
                .is_some_and(|id| locate::group_holds(&button_key, g, id));
            collapsed_button(
                g.title,
                first_tool_icon(g),
                self.layout.show_panel_titles,
                tight,
                self.collapsed_open.as_deref() == Some(g.title),
                holds,
            )
        };
        Panel::deferred(g.title, move |level| match level {
            collapse::Level::Full => group(false),
            collapse::Level::Compact => group(true),
            collapse::Level::Collapsed => button(false),
            collapse::Level::Tight => button(true),
        })
    }

    pub fn toggle_dropdown(&mut self, id: &str) {
        // A slide-out tool's submenu that was open goes.
        self.slide.closed(false);
        // The extra tools of a peek's panel open inside the peek.
        if self.peek_extras().is_some_and(|panel| panel.id == id) {
            self.toggle_peek_extras();
            return;
        }
        if self.open_dropdown.as_deref() == Some(id) {
            self.open_dropdown = None;
        } else {
            self.open_dropdown = Some(id.to_string());
            // Another panel's slide-out closes the peek.
            if draw_panel::panel_for_dropdown(id).is_some_and(|panel| panel.id == id) {
                self.close_popup();
            }
        }
    }
    pub fn close_dropdown(&mut self) {
        self.open_dropdown = None;
        self.collapsed_open = None;
        self.layer_filter.clear();
        self.slide.closed(true);
    }

    pub fn escape_extension(&mut self) -> bool {
        let Some(id) = self.open_dropdown.as_deref() else {
            return false;
        };
        let Some(parent) = draw_panel::parent_panel(id) else {
            return false;
        };
        // From a submenu back to its slide-out, unless that one is pinned
        // and shows anyway.
        let pinned = draw_panel::panel_for_dropdown(parent)
            .is_some_and(|panel| self.layout.pinned_slideouts.contains(panel.key));
        self.slide.closed(false);
        // In a peek, from a submenu back to the peek.
        if self.collapsed_open.is_some() {
            self.open_dropdown = None;
        } else if id == parent || pinned {
            self.close_dropdown();
        } else {
            self.open_dropdown = Some(parent.to_string());
        }
        true
    }

    /// Toggle the peek of a collapsed ribbon panel (identified by its title).
    pub fn toggle_collapsed_panel(&mut self, id: &str) {
        if self.collapsed_open.as_deref() == Some(id) {
            self.close_popup();
        } else {
            self.close_popup();
            self.collapsed_open = Some(id.to_string());
        }
    }

    /// Returns the index of the Layout module in the modules list. Kept as a
    /// routing helper for when the currently suppressed Layout ribbon tab is
    /// enabled; it has no effect while that module is absent from the tabs.
    #[allow(dead_code)]
    pub fn layout_module_index(&self) -> Option<usize> {
        self.modules.iter().position(|m| m.id() == "layout")
    }

    /// Returns true if the currently active tab is the Layout module. The
    /// predicate remains useful to the future Layout tab, which is currently
    /// suppressed by the view layer.
    #[allow(dead_code)]
    pub fn active_is_layout(&self) -> bool {
        self.modules
            .get(self.active)
            .map(|m| m.id() == "layout")
            .unwrap_or(false)
    }

    pub fn select_dropdown_item(&mut self, dropdown_id: &'static str, cmd: &'static str) {
        self.last_cmd.insert(dropdown_id, cmd);
        self.close_dropdown();
    }

    /// Show `cmd` as the current item of a dropdown that mirrors a drawing
    /// setting, without running it.
    pub fn set_dropdown_current(&mut self, dropdown_id: &'static str, cmd: &'static str) {
        if self.last_cmd.get(dropdown_id) != Some(&cmd) {
            self.last_cmd.insert(dropdown_id, cmd);
        }
    }

    // ── View ──────────────────────────────────────────────────────────────

    /// The ribbon. With `menu_row` (MENUBAR 1) the large logo spans the
    /// menu row and the tab row; without it a compact logo starts the tab
    /// row. A `closed` ribbon (RIBBONCLOSE) keeps the tab row.
    #[allow(clippy::too_many_arguments)]
    pub fn view<'a>(
        &'a self,
        menu_row: Option<Element<'a, Message>>,
        closed: bool,
        is_paper: bool,
        is_start: bool,
        undo_count: usize,
        redo_count: usize,
        show_block_palette: bool,
        visibility: Option<crate::ui::window::block_visibility::RibbonContext>,
    ) -> Element<'a, Message> {
        // ── Quick-access file commands + undo/redo, one merged flow ────────
        let lead = iced::widget::Row::with_children(
            QUICK_ACCESS
                .iter()
                .map(|&(icon, label, cmd)| quick_access_btn(icon, label, cmd))
                .chain([
                    render_history_control(
                        "Undo",
                        UNDO_HISTORY_ID,
                        undo_count,
                        &self.open_dropdown,
                    ),
                    render_history_control(
                        "Redo",
                        REDO_HISTORY_ID,
                        redo_count,
                        &self.open_dropdown,
                    ),
                ]),
        )
        .spacing(TOP_HIST_GAP)
        .align_y(iced::Center)
        .wrap()
        .vertical_spacing(0.0);

        // The quick-access flow and the tabs flow each flex-wrap; WrapBar stacks
        // them so a wrapped tab never shares a row with a quick-access button.

        // A running command whose button is on another tab tints that tab's
        // header with a faint wash of the button's blue, pointing at where the
        // button lives without competing with the selected tab's outline.
        let holding_tab = self.tab_holding_active_tool();
        let tab_items = self.modules.iter().enumerate().fold(
            Vec::<Element<'_, Message>>::new(),
            |mut acc, (i, module)| {
                // The Layout module no longer has a ribbon tab — its paper-space
                // tools live in the right-edge side toolbar (see ui::side_toolbar).
                if module.id() == "layout"
                    || (module.id() == "block_editor" && self.block_editing.is_none())
                    || self.tab_hidden(i)
                {
                    return acc;
                }

                let is_active = i == self.active;
                let is_contextual = is_contextual(module.id());
                let holds_tool = holding_tab == Some(i);
                let tab = button(
                    container(
                        text(crate::i18n::ribbon_module_title(
                            module.id(),
                            module.title(),
                        ))
                        .size(TAB_TEXT_SIZE)
                        .wrapping(iced::widget::text::Wrapping::None),
                    )
                    .center_y(Fill),
                )
                .on_press(Message::RibbonSelectTab(i))
                .style(move |theme: &Theme, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    let background = if is_active || hovered {
                        Some(chrome::panel(theme))
                    } else if holds_tool {
                        Some(chrome::lit(theme))
                    } else {
                        None
                    };
                    button::Style {
                        background: background.map(Background::Color),
                        text_color: if is_active || hovered || holds_tool {
                            chrome::strong(theme)
                        } else {
                            chrome::tab_text(theme)
                        },
                        ..Default::default()
                    }
                })
                .height(TAB_H)
                .padding([0, 9]);
                // The active tab shares the tool area's colour and opens into
                // it. A contextual tab is outlined on three sides, also while
                // another tab is on show, so it is easy to find again.
                let tab: Element<'_, Message> = if is_contextual {
                    tab_outline(tab.into())
                } else {
                    tab.into()
                };
                // Tabs stand on the bottom of the tab row, on the tool area.
                let btn = container(tab).height(TAB_ROW_H).align_bottom(TAB_ROW_H);
                // Minimized to tabs, resting on a tab opens its tool area.
                let btn: Element<'_, Message> = if self.layout.minimize == RibbonMinimize::Tabs {
                    mouse_area(btn)
                        .on_enter(Message::RibbonMinimize(MinimizeMsg::EnterOpener(
                            Opener::Tab(i),
                        )))
                        .on_exit(Message::RibbonMinimize(MinimizeMsg::LeaveOpener(
                            Opener::Tab(i),
                        )))
                        .into()
                } else {
                    btn.into()
                };
                acc.push(keytips::anchor_with(|| format!("tab:{}", module.id()), btn));
                acc
            },
        );
        let mut tab_items = tab_items;
        // A minimized ribbon has no tool row to be tight.
        if self.layout.minimize != RibbonMinimize::Off
            || !self.collapse_tight.load(Ordering::Relaxed)
        {
            tab_items.push(self.minimize_button());
        }

        // Tabs may squeeze their gaps to fit before wrapping: from the normal 6px
        // down to -12px on a narrow (e.g. phone) tab row, tucking neighbours into
        // each other's 14px side padding without overlapping the labels. When the
        // row has room the gap stays 6px, so wide/desktop layouts are unchanged.
        let tabs = WrapFlow::new(tab_items)
            .spacing_x(metrics::TAB_GAP)
            .min_spacing_x(-6.0)
            .row_h(TAB_ROW_H);

        let mut tab_row = row![].spacing(6.0).align_y(iced::Center);
        if menu_row.is_none() {
            tab_row = tab_row.push(self.logo_button(false));
        }
        tab_row = tab_row.push(
            container(
                WrapBar::new(lead.into(), tabs.into())
                    .spacing(6.0)
                    .min_row_h(TAB_ROW_H)
                    .report_height(self.tab_bar_h.clone()),
            )
            .width(Length::Fill),
        );
        // Without the menu row the search box ends the tab row.
        if menu_row.is_none() {
            tab_row = tab_row.push(self.search_box());
        }

        let tab_bar = container(tab_row)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::bar(theme))),
                ..Default::default()
            })
            .padding(Padding {
                right: 6.0,
                ..Padding::ZERO
            })
            .width(Length::Fill);

        let tab_bar = PosReport::new(minimize::TAB_BAR_ID, tab_bar);
        let top: Element<'a, Message> = match menu_row {
            Some(menu_row) => row![
                container(self.logo_button(true)).style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(crate::ui::style::chrome::bar(theme))),
                    ..Default::default()
                }),
                column![self.with_search_box(menu_row), tab_bar].width(Fill),
            ]
            .into(),
            None => tab_bar.into(),
        };
        let mut ribbon = column![top];
        match self.layout.minimize {
            _ if closed => {}
            RibbonMinimize::Off => {
                ribbon = ribbon.push(self.tool_area(
                    is_paper,
                    is_start,
                    show_block_palette,
                    visibility,
                    self.collapse_mode,
                ));
            }
            // The tab's tool area opens over the drawing (`minimized_popup`).
            RibbonMinimize::Tabs => {}
            RibbonMinimize::PanelTitles | RibbonMinimize::PanelButtons => {
                ribbon = ribbon.push(self.minimized_row(is_start));
            }
        }
        keytips::ribbon_area(ribbon_menu::RightPress::new(ribbon))
    }

    /// The menu row with the search box at its right end.
    pub fn with_search_box<'a>(&'a self, menu_row: Element<'a, Message>) -> Element<'a, Message> {
        container(
            row![
                container(menu_row).width(Fill),
                container(self.search_box()).padding([0, 6]),
            ]
            .align_y(iced::Center),
        )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(crate::ui::style::chrome::bar(theme))),
            ..Default::default()
        })
        .into()
    }

    /// The style lists the style combos show.
    fn style_context(&self) -> StyleContext<'_> {
        StyleContext {
            text_style_names: &self.text_style_names,
            active_text_style: &self.active_text_style,
            dim_style_names: &self.dim_style_names,
            active_dim_style: &self.active_dim_style,
            mleader_style_names: &self.mleader_style_names,
            active_mleader_style: &self.active_mleader_style,
            table_style_names: &self.table_style_names,
            active_table_style: &self.active_table_style,
        }
    }

    /// The tool area of the tab on show: its panels, sized by `mode`.
    fn tool_area(
        &self,
        is_paper: bool,
        is_start: bool,
        show_block_palette: bool,
        visibility: Option<crate::ui::window::block_visibility::RibbonContext>,
        mode: CollapseMode,
    ) -> Element<'_, Message> {
        // ── Tool area ─────────────────────────────────────────────────────
        let effective_active = if !is_paper
            && self
                .modules
                .get(self.active)
                .map(|m| m.id() == "layout")
                .unwrap_or(false)
        {
            0
        } else {
            self.active
        };
        let tool_area: Element<'_, Message> = if self.modules.get(effective_active).is_some() {
            let groups: Vec<_> = self.groups_for(effective_active).collect();
            let style_ctx = self.style_context();

            // Adaptive tool area: panels sit on one row; when they don't all
            // fit they degrade from the right — a panel's large buttons first
            // shrink to compact icon columns, then it collapses to a ▾ flyout
            // button. See `CollapsePanels`.
            let ts = self.toggle_state(show_block_palette);
            let measure_key = if mode == CollapseMode::Auto {
                // Panels without titles are shorter: measure them afresh.
                let mut hasher = DefaultHasher::new();
                panel_measure_key(&groups, &self.last_cmd, &style_ctx, visibility.as_ref())
                    .hash(&mut hasher);
                self.layout.show_panel_titles.hash(&mut hasher);
                hasher.finish()
            } else {
                0
            };
            let panels: Vec<Panel<'_>> = groups
                .into_iter()
                .map(|g| {
                    if self.modules[effective_active].id() == "block_editor"
                        && g.title == "Visibility"
                    {
                        visibility_panel::panel(
                            visibility.clone(),
                            self.layout.show_panel_titles,
                            self.collapsed_open.as_deref() == Some(g.title),
                        )
                    } else {
                        self.panel(self.modules[effective_active].id(), g, ts, style_ctx)
                    }
                })
                .collect();
            // Panels drag by their title strips; without titles they stay put.
            let module = self.modules[effective_active].id();
            let strip = if self.layout.show_panel_titles {
                PANEL_TITLE_H
            } else {
                0.0
            };
            CollapsePanels::new(panels, self.panel_h())
                .draggable(strip, move |titles| Message::RibbonReorderPanels {
                    module,
                    titles,
                })
                .measure_key(measure_key)
                .report_height(self.tool_bar_h.clone())
                .report_tight(self.collapse_tight.clone())
                .mode(mode)
                .into()
        } else {
            text("").into()
        };

        // The tool area, with a line of `page` under it.
        let tool_bar: Element<'_, Message> = column![
            container(tool_area)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(chrome::panel(theme))),
                    ..Default::default()
                })
                .width(Length::Fill),
            container(iced::widget::Space::new().width(Fill).height(1)).style(|theme: &Theme| {
                container::Style {
                    background: Some(Background::Color(chrome::page(theme))),
                    ..Default::default()
                }
            }),
        ]
        .into();
        // A contextual tab's tool area has an accent line along its top,
        // drawn over the area's own border so its height stays.
        let tool_bar = if self
            .modules
            .get(effective_active)
            .is_some_and(|module| is_contextual(module.id()))
        {
            let line = container(
                iced::widget::Space::new()
                    .width(Fill)
                    .height(CONTEXTUAL_LINE),
            )
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(crate::ui::style::chrome::accent(theme))),
                ..Default::default()
            });
            iced::widget::stack![tool_bar, line].into()
        } else {
            tool_bar
        };

        if is_start {
            // The Start tab has no drawing context. One theme-aware shield
            // fades and blocks only the tool area; the tabs and application
            // controls above it remain available and clearly identifiable.
            let shield = mouse_area(
                container(iced::widget::Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(|theme: &Theme| container::Style {
                        background: Some(Background::Color(chrome::page(theme).scale_alpha(0.6))),
                        ..Default::default()
                    }),
            )
            .on_press(Message::CloseRibbonDropdown)
            .interaction(iced::mouse::Interaction::Idle);
            let shield = iced::widget::tooltip(
                shield,
                make_tip(t!("Open or create a drawing to use ribbon commands.").into_owned()),
                iced::widget::tooltip::Position::Bottom,
            )
            .gap(6.0)
            .delay(std::time::Duration::from_millis(400))
            .style(tip_style);

            iced::widget::stack![tool_bar, iced::widget::opaque(shield)].into()
        } else {
            tool_bar
        }
    }

    // ── Dropdown overlay ──────────────────────────────────────────────────

    /// Place `panel` (`w` wide) under dropdown `id`, closing on a click
    /// outside it.
    pub fn place_dropdown<'a>(
        &self,
        id: &str,
        panel: Element<'a, Message>,
        w: f32,
        win_w: f32,
    ) -> Element<'a, Message> {
        let (align_right, h_pad, top) = self.dd_anchor(id, w, win_w);
        dropdown_backdrop(position_ribbon_dropdown(panel, align_right, h_pad, top))
    }

    pub fn dropdown_overlay(
        &self,
        undo_labels: &[String],
        redo_labels: &[String],
        win: (f32, f32),
        is_start: bool,
        recent_colors: &[AcadColor],
        recent_files: &[std::path::PathBuf],
    ) -> Option<Element<'_, Message>> {
        let open_id = self.open_dropdown.as_deref()?;
        if open_id == APP_MENU_ID {
            return Some(self.app_menu(recent_files, win));
        }
        // The minimize button belongs to the tab row, which the Start tab
        // keeps; the tools' dropdowns need a drawing.
        if open_id == minimize::MINIMIZE_MENU_ID {
            return Some(self.minimize_menu(win));
        }
        if is_start {
            return None;
        }

        if draw_panel::owns_dropdown(open_id) {
            return Some(draw_panel::overlay(self, open_id, win));
        }

        if open_id == UNDO_HISTORY_ID || open_id == REDO_HISTORY_ID {
            let is_undo = open_id == UNDO_HISTORY_ID;
            let labels = if is_undo { undo_labels } else { redo_labels };
            if labels.is_empty() {
                return None;
            }

            let rows: Vec<Element<Message>> = labels
                .iter()
                .enumerate()
                .map(|(idx, label)| {
                    let step = idx + 1;
                    button(text(label.clone()).size(11))
                        .on_press(if is_undo {
                            Message::UndoMany(step)
                        } else {
                            Message::RedoMany(step)
                        })
                        .style(popup_row_style)
                        .width(Fill)
                        .padding([5, 10])
                        .into()
                })
                .collect();

            let panel = container(column(rows))
                .style(popup_panel_style)
                .width(Length::Fixed(170.0));

            let (align_right, h_pad, top) = self.dd_anchor(open_id, 170.0, win.0);
            let positioned = position_ribbon_dropdown(panel.into(), align_right, h_pad, top);

            return Some(dropdown_backdrop(positioned));
        }

        if open_id == LAYER_COMBO_ID {
            return self.layer_combo_overlay(win);
        }

        if open_id == PROP_COLOR_ID {
            return self.prop_color_overlay(win, recent_colors);
        }
        if open_id == PROP_LINETYPE_ID {
            return self.prop_linetype_overlay(win);
        }
        if open_id == PROP_LW_ID {
            return self.prop_lw_overlay(win);
        }

        // Style combo dropdowns (annotate tab) float as overlays so the list
        // isn't clipped by the fixed ribbon-row height, the way the Draw-tab
        // dropdowns already are. (#153)
        if let Some(ov) = self.style_combo_overlay(open_id, win) {
            return Some(ov);
        }

        let groups = self.groups_for(self.active);
        let mut items_list: Option<Vec<(&'static str, &'static str, IconKind)>> = None;
        let mut dd_default = "";
        let mut dd_id: &'static str = "";

        'outer: for group in groups {
            for item in &group.tools {
                let (id, items, default) = match item {
                    RibbonItem::Dropdown {
                        id, items, default, ..
                    } => (*id, items, *default),
                    RibbonItem::LabeledDropdown {
                        id, items, default, ..
                    } => (*id, items, *default),
                    RibbonItem::LargeDropdown {
                        id, items, default, ..
                    } => (*id, items, *default),
                    _ => continue,
                };
                if id == open_id {
                    items_list = Some(items.clone());
                    dd_default = default;
                    dd_id = id;
                    break 'outer;
                }
            }
        }
        let items = items_list?;
        let last_cmd = self.last_cmd.get(dd_id).copied().unwrap_or(dd_default);

        let rows: Vec<Element<Message>> = items
            .iter()
            .map(|(cmd, label, item_icon)| {
                let is_current = *cmd == last_cmd;
                let disabled = widgets::is_disabled_item(cmd);
                let checkmark: Element<'_, Message> =
                    crate::ui::icons::themed_check_cell(is_current);
                let icon_el: Element<Message> = container(make_icon(*item_icon, SMALL_ICON))
                    .width(Length::Fixed(SMALL_ICON))
                    .into();
                let label_el = text(t!(*label))
                    .size(11)
                    .wrapping(iced::advanced::text::Wrapping::None)
                    .style(move |theme: &Theme| iced::widget::text::Style {
                        color: (!is_current || disabled).then_some(chrome::muted(theme)),
                    });
                let label_el = keytips::label_anchor(
                    || keytips::label_id(&keytips::item_anchor(dd_id, cmd)),
                    label_el,
                );

                let item = button(
                    row![checkmark, icon_el, label_el]
                        .spacing(4)
                        .align_y(iced::Center),
                )
                .on_press_maybe((!disabled).then_some(Message::DropdownSelectItem {
                    dropdown_id: dd_id,
                    cmd: *cmd,
                }))
                .style(popup_row_style)
                .width(Fill)
                .padding([4, 10]);
                keytips::anchor_with(|| keytips::item_anchor(dd_id, cmd), item)
            })
            .collect();

        let labels: Vec<String> = items
            .iter()
            .map(|(_, label, _)| t!(*label).to_string())
            .collect();
        let header = dropdown_header(dd_id).map(|header| t!(header).into_owned());
        let mut rows = rows;
        if let Some(header) = &header {
            rows.insert(
                0,
                container(text(header.clone()).size(10).style(muted_text_style))
                    .padding([4, 10])
                    .into(),
            );
        }
        let panel_w = crate::ui::style::common::dropdown_popup_width(
            labels.iter().map(|s| s.as_str()),
            11.0,
            68.0,
            // A header wraps rather than widening the menu.
            if header.is_some() { 240.0 } else { 190.0 },
        );

        let panel = container(column(rows))
            .style(popup_panel_style)
            .width(Length::Fixed(panel_w));

        let (align_right, h_pad, top) = self.dd_anchor(open_id, panel_w, win.0);
        Some(dropdown_backdrop(position_ribbon_dropdown(
            panel.into(),
            align_right,
            h_pad,
            top,
        )))
    }

    fn layer_combo_overlay(&self, win: (f32, f32)) -> Option<Element<'_, Message>> {
        // A toggle icon (visible / freeze / lock) is its own button so a click
        // on it flips that state instead of bubbling up to the row's
        // make-active handler (#133).
        let icon_btn = |bytes: &'static [u8], msg: Message| -> Element<'_, Message> {
            button(crate::ui::icons::tool(bytes, SMALL_ICON))
                .on_press(msg)
                .style(popup_row_style)
                .padding([2, 4])
                .into()
        };
        let filter = self.layer_filter.to_lowercase();
        let rows: Vec<Element<Message>> = self
            .layer_infos
            .iter()
            .filter(|info| filter.is_empty() || info.name.to_lowercase().contains(&filter))
            .map(|info| {
                let is_active = info.name == self.active_layer;
                let lc = info.color;
                let lv = info.visible;
                let lf = info.frozen;
                let ll = info.locked;
                let name = info.name.clone();

                let swatch = container(text(""))
                    .style(move |theme: &Theme| container::Style {
                        background: Some(Background::Color(lc)),
                        border: Border {
                            color: chrome::border(theme),
                            width: 1.0,
                            radius: 1.0.into(),
                        },
                        ..Default::default()
                    })
                    .width(12)
                    .height(12);

                let vis = icon_btn(
                    crate::ui::icons::layer_visible(lv),
                    Message::LayerToggleVisible(info.name.clone()),
                );
                let freeze = icon_btn(
                    crate::ui::icons::layer_freeze(lf),
                    Message::LayerToggleFreeze(info.name.clone()),
                );
                let lock = icon_btn(
                    crate::ui::icons::layer_lock(ll),
                    Message::LayerToggleLock(info.name.clone()),
                );
                // Freeze in the current viewport only (#1466).
                let vp_freeze = info.vp_frozen.map(|(column, frozen)| {
                    icon_btn(
                        crate::ui::icons::layer_vp_freeze(frozen),
                        Message::LayerToggleVpFreeze(info.name.clone(), column),
                    )
                });
                let checkmark: Element<'_, Message> =
                    crate::ui::icons::themed_check_cell(is_active);
                let label = text(&info.name).size(11).style(move |theme: &Theme| {
                    iced::widget::text::Style {
                        color: (!is_active).then_some(chrome::muted(theme)),
                    }
                });

                // The swatch + label area selects the layer as active; the
                // icon buttons above handle their own toggles.
                let select = button(row![swatch, label].spacing(5).align_y(iced::Center))
                    .on_press(Message::RibbonLayerChanged(name))
                    .style(popup_row_style)
                    .width(Fill)
                    .padding([4, 4]);

                container(
                    row![checkmark, vis, freeze]
                        .push(vp_freeze)
                        .push(lock)
                        .push(select)
                        .spacing(5)
                        .align_y(iced::Center),
                )
                .padding([0, 4])
                .into()
            })
            .collect();

        // Cap the panel height and make the list scrollable so a long layer
        // list stays reachable instead of running off the bottom of the
        // screen (#227). Short lists shrink to fit; longer ones scroll.
        let row_count = rows.len().max(1);
        let list_h = (row_count as f32 * 26.0).min(420.0);
        // Search box (#343): filters the list live as the user types.
        let search = iced::widget::text_input(t!("Search layers…").as_ref(), &self.layer_filter)
            .on_input(Message::RibbonLayerFilterChanged)
            .size(11)
            .padding([4, 6]);
        let state_manager = button(
            row![
                crate::ui::icons::themed_arrow_right(9.0),
                text(t!("Layer State Manager…")).size(11),
            ]
            .spacing(7)
            .align_y(iced::Center),
        )
        .on_press(Message::LayerStateManagerOpen)
        .style(popup_row_style)
        .width(Fill)
        .padding([6, 10]);
        let panel = container(
            column![
                container(search).padding([4, 4]),
                scrollable(column(rows)).height(Length::Fixed(list_h)),
                container(state_manager)
                    .width(Fill)
                    .padding([3, 4])
                    .style(|theme: &Theme| container::Style {
                        border: Border {
                            color: chrome::border(theme),
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    }),
            ]
            .spacing(2),
        )
        .style(popup_panel_style)
        .width(Length::Fixed(300.0));

        let (align_right, h_pad, top) = self.dd_anchor(LAYER_COMBO_ID, 300.0, win.0);
        let positioned = position_ribbon_dropdown(panel.into(), align_right, h_pad, top);

        Some(dropdown_backdrop(positioned))
    }

    /// Floating popup for an annotate-tab style combo (text / dimension /
    /// multileader / table style). Returns `None` when `open_id` is not a
    /// style combo. Built as an overlay — like the layer combo and the
    /// Draw-tab dropdowns — so the list grows to fit its entries instead of
    /// being clipped to the ribbon-row height. (#153)
    fn style_combo_overlay(&self, open_id: &str, win: (f32, f32)) -> Option<Element<'_, Message>> {
        let groups = self.groups_for(self.active);

        // Locate the open style combo; capture its style key + manager command.
        let mut found: Option<(crate::modules::StyleKey, Option<&'static str>)> = None;
        'outer: for group in groups {
            for item in &group.tools {
                if let RibbonItem::StyleComboGroup {
                    style_key,
                    combo_id,
                    manager_cmd,
                    ..
                } = item
                {
                    if *combo_id == open_id {
                        found = Some((*style_key, *manager_cmd));
                        break 'outer;
                    }
                }
            }
        }
        let (style_key, manager_cmd) = found?;

        let ctx = StyleContext {
            text_style_names: &self.text_style_names,
            active_text_style: &self.active_text_style,
            dim_style_names: &self.dim_style_names,
            active_dim_style: &self.active_dim_style,
            mleader_style_names: &self.mleader_style_names,
            active_mleader_style: &self.active_mleader_style,
            table_style_names: &self.table_style_names,
            active_table_style: &self.active_table_style,
        };
        let active = ctx.active_for(style_key).to_string();

        let mut rows: Vec<Element<Message>> = ctx
            .names_for(style_key)
            .iter()
            .map(|name| {
                let is_sel = name.as_str() == active.as_str();
                let n = name.clone();
                let checkmark: Element<Message> = crate::ui::icons::themed_check_cell(is_sel);
                button(
                    row![
                        checkmark,
                        text(name.clone()).size(11).style(move |theme: &Theme| {
                            iced::widget::text::Style {
                                color: (!is_sel).then_some(chrome::muted(theme)),
                            }
                        }),
                    ]
                    .spacing(4)
                    .align_y(iced::Center),
                )
                .on_press(Message::RibbonStyleChanged {
                    key: style_key,
                    name: n,
                })
                .style(popup_row_style)
                .width(Fill)
                .padding([4, 10])
                .into()
            })
            .collect();

        if let Some(mgr) = manager_cmd {
            rows.push(
                button(text(t!("Manage…")).size(11))
                    .on_press(Message::Command(mgr.to_string()))
                    .style(popup_row_style)
                    .width(Fill)
                    .padding([4, 10])
                    .into(),
            );
        }

        let panel = container(column(rows))
            .style(popup_panel_style)
            .width(Length::Fixed(LARGE_W * 2.3));

        let (align_right, h_pad, top) = self.dd_anchor(open_id, LARGE_W * 2.3, win.0);
        Some(dropdown_backdrop(position_ribbon_dropdown(
            panel.into(),
            align_right,
            h_pad,
            top,
        )))
    }

    fn prop_color_overlay(
        &self,
        win: (f32, f32),
        recent_colors: &[AcadColor],
    ) -> Option<Element<'_, Message>> {
        let panel = color_dropdown::color_dropdown_panel(
            self.active_color,
            recent_colors,
            self.active_layer_color(),
        );

        let (align_right, h_pad, top) =
            self.dd_anchor(PROP_COLOR_ID, color_dropdown::PANEL_W, win.0);
        Some(dropdown_backdrop(position_ribbon_dropdown(
            panel,
            align_right,
            h_pad,
            top,
        )))
    }

    fn prop_linetype_overlay(&self, win: (f32, f32)) -> Option<Element<'_, Message>> {
        let active_lt = &self.active_linetype;

        let mut items: Vec<LinetypeItem> = vec![
            LinetypeItem {
                name: "ByLayer".to_string(),
                art: String::new(),
            },
            LinetypeItem {
                name: "ByBlock".to_string(),
                art: String::new(),
            },
        ];
        for lt in &self.available_linetypes {
            if lt.name != "ByLayer" && lt.name != "ByBlock" {
                items.push(lt.clone());
            }
        }

        let rows: Vec<Element<Message>> = items
            .into_iter()
            .map(|lt| {
                let is_cur = lt.name == *active_lt;
                let check: Element<'_, Message> = crate::ui::icons::themed_check_cell(is_cur);
                let name_col = text(linetype_display_name(&lt.name))
                    .size(11)
                    .style(move |theme: &Theme| iced::widget::text::Style {
                        color: (!is_cur).then_some(chrome::muted(theme)),
                    })
                    .width(Length::Fixed(90.0));
                let art_col = text(lt.art.clone()).size(9).style(muted_text_style);
                let name = lt.name.clone();
                button(
                    row![check, name_col, art_col]
                        .spacing(4)
                        .align_y(iced::Center),
                )
                .on_press(Message::RibbonLinetypeChanged(name))
                .style(popup_row_style)
                .width(Fill)
                .padding([4, 6])
                .into()
            })
            .collect();

        let list = container(scrollable(column(rows)).height(Length::Fixed(200.0)))
            .style(popup_panel_style)
            .width(Length::Fixed(220.0));

        let (align_right, h_pad, top) = self.dd_anchor(PROP_LINETYPE_ID, 220.0, win.0);
        Some(dropdown_backdrop(position_ribbon_dropdown(
            list.into(),
            align_right,
            h_pad,
            top,
        )))
    }

    fn prop_lw_overlay(&self, win: (f32, f32)) -> Option<Element<'_, Message>> {
        let active_lw = self.active_lineweight;
        let rows: Vec<Element<Message>> = lw_options()
            .into_iter()
            .map(|item| {
                let is_cur = item.0 == active_lw;
                let label = item.to_string();
                let check: Element<'_, Message> = crate::ui::icons::themed_check_cell(is_cur);
                button(
                    row![
                        check,
                        text(label).size(11).style(move |theme: &Theme| {
                            iced::widget::text::Style {
                                color: (!is_cur).then_some(chrome::muted(theme)),
                            }
                        })
                    ]
                    .spacing(5)
                    .align_y(iced::Center),
                )
                .on_press(Message::RibbonLineweightChanged(item.0))
                .style(popup_row_style)
                .width(Fill)
                .padding([4, 8])
                .into()
            })
            .collect();

        self.prop_overlay_positioned(rows, PROP_LW_ID, 140.0, win)
    }

    fn prop_overlay_positioned<'a>(
        &'a self,
        rows: Vec<Element<'a, Message>>,
        dd_id: &str,
        width: f32,
        win: (f32, f32),
    ) -> Option<Element<'a, Message>> {
        let panel = container(column(rows))
            .style(popup_panel_style)
            .width(Length::Fixed(width));

        let (align_right, h_pad, top) = self.dd_anchor(dd_id, width, win.0);
        Some(dropdown_backdrop(position_ribbon_dropdown(
            panel.into(),
            align_right,
            h_pad,
            top,
        )))
    }
}

/// How a panel shows its title strip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Strip {
    /// The title strip, with the title.
    Titled,
    /// Panel titles are off.
    Untitled,
    /// No strip: the body of a collapsed or minimized panel's peek, whose
    /// footer stands for the strip.
    Peek,
}

/// Render a single ribbon panel (tools + group label), fixed `TOOL_BAR_H` tall.
/// When `compact`, large tools/dropdowns are drawn as small icon columns.
#[allow(clippy::too_many_arguments)]
fn render_group<'a>(
    compact: bool,
    group: &RibbonGroup,
    key: &str,
    strip: Strip,
    look: draw_panel::SlideLook,
    lead: Vec<Element<'a, Message>>,
    active_tool: &Option<String>,
    open_dd: &Option<String>,
    last_cmd: &HashMap<&'static str, &'static str>,
    state: widgets::ToggleState,
    layer_infos: &'a [LayerInfo],
    active_layer: &'a str,
    active_color: AcadColor,
    active_linetype: &'a str,
    active_lineweight: LineWeight,
    style_ctx: &StyleContext<'_>,
) -> Element<'a, Message> {
    let mut items_row: Vec<Element<Message>> = lead;
    let mut small_buf: Vec<Element<Message>> = Vec::new();

    let ctx = widgets::RenderCtx {
        active_tool,
        open_dd,
        last_cmd,
        state,
        layer_infos,
        active_layer,
        active_color,
        active_linetype,
        active_lineweight,
        style_ctx,
        compact,
    };

    for item in &group.tools {
        let is_large = match item {
            RibbonItem::LargeTool(_) | RibbonItem::LargeDropdown { .. } => !compact,
            RibbonItem::ToolGrid { .. }
            | RibbonItem::LayerComboGroup { .. }
            | RibbonItem::PropertiesGroup { .. }
            | RibbonItem::StyleComboGroup { .. } => true,
            _ => false,
        };

        if is_large {
            flush_small_col(&mut small_buf, &mut items_row);
            items_row.push(render_large(item, &ctx));
        } else {
            small_buf.push(render_small(item, active_tool, open_dd, last_cmd, state));
            if small_buf.len() == 3 {
                flush_small_col(&mut small_buf, &mut items_row);
            }
        }
    }
    flush_small_col(&mut small_buf, &mut items_row);

    let tools_el = items_row.into_iter().fold(
        row![]
            .spacing(2)
            .height(Length::Fixed(LARGE_H))
            .align_y(iced::Top),
        |r, e| r.push(e),
    );

    // The tools sit 3 px from the title strip's top line and the tool area's
    // top edge; without titles they are centred in the shorter area.
    let body = container(tools_el).padding(Padding {
        top: metrics::CONTENT_PAD_TOP,
        right: metrics::CONTENT_PAD_SIDE,
        bottom: metrics::CONTENT_PAD_BOTTOM,
        left: metrics::CONTENT_PAD_SIDE,
    });
    let body = match strip {
        Strip::Titled | Strip::Peek => body.height(metrics::CONTENT_H),
        Strip::Untitled => body.height(TOOL_BAR_H_UNTITLED).align_y(iced::Center),
    };
    let mut panel = column![body];
    let mut launcher = None;
    let mut press = None;
    if strip == Strip::Titled {
        let title = draw_panel::group_title(group.title, key, look);
        panel = panel.push(title.row);
        launcher = title.launcher;
        press = title.press;
    }
    let height = match strip {
        Strip::Titled => TOOL_BAR_H,
        Strip::Untitled => TOOL_BAR_H_UNTITLED,
        Strip::Peek => metrics::CONTENT_H,
    };
    let panel = panel
        .align_x(iced::Center)
        .spacing(0)
        .height(Length::Fixed(height));
    let strip_h = if strip == Strip::Titled {
        PANEL_TITLE_H
    } else {
        0.0
    };
    let panel = panel_frame::PanelFrame::new(panel.into(), strip_h, launcher)
        .lit(look == draw_panel::SlideLook::Open)
        .on_press(press);
    // A peek shows the extra tools inside itself.
    if strip == Strip::Peek {
        return panel.into();
    }
    let Some(extras) = draw_panel::panel_for_group(key) else {
        return panel.into();
    };
    // A panel with a slide-out reports its body, which the slide-out opens
    // under, and where the pointer is, which closes it. While it is open
    // and not pinned, the panel is lit like the slide-out.
    let panel = container(panel).style(move |theme: &Theme| container::Style {
        background: (look == draw_panel::SlideLook::Open)
            .then(|| Background::Color(crate::ui::style::chrome::open_panel(theme))),
        ..Default::default()
    });
    let panel = mouse_area(panel)
        .on_enter(Message::RibbonSlideOut(
            draw_panel::SlideOutMsg::EnterPanel(extras.key),
        ))
        .on_exit(Message::RibbonSlideOut(
            draw_panel::SlideOutMsg::LeavePanel(extras.key),
        ));
    PosReport::owned(draw_panel::panel_box_id(key), panel).into()
}

/// The icon of a panel's first tool-like item, which its collapsed button and
/// minimized panel button show; digs into the composite groups so
/// Layers/Properties/Styles panels still get an icon.
fn first_tool_icon(group: &RibbonGroup) -> Option<IconKind> {
    group.tools.iter().find_map(|it| match it {
        RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) | RibbonItem::LargeTool(t) => Some(t.icon),
        RibbonItem::Dropdown { icon, .. }
        | RibbonItem::LabeledDropdown { icon, .. }
        | RibbonItem::LargeDropdown { icon, .. } => Some(*icon),
        RibbonItem::PropertiesGroup { match_prop } => Some(match_prop.icon),
        RibbonItem::LayerComboGroup { row2, .. } => row2.first().map(|t| t.icon),
        RibbonItem::StyleComboGroup { rows, .. } => {
            rows.first().and_then(|r| r.first()).map(|t| t.icon)
        }
        RibbonItem::ToolGrid { columns } => columns
            .first()
            .and_then(|column| column.first())
            .map(|tool| tool.icon),
    })
}

/// An auto-collapsed panel: one large button with the icon of the panel's
/// first tool, its title and a ▾, as high as the tool area. A click, or the
/// pointer resting on it, opens the panel's peek under it (`minimized_popup`).
/// In the `tight` step the title wraps onto two lines, so the button is
/// narrower. It lights while one of the panel's tools runs (`holds`) and
/// shows hovered while its peek is `open`.
pub(super) fn collapsed_button<'a>(
    title: &'static str,
    icon: Option<IconKind>,
    titles: bool,
    tight: bool,
    open: bool,
    holds: bool,
) -> Element<'a, Message> {
    let localized_title = t!(title).into_owned();
    let icon: Element<'_, Message> = match icon {
        Some(icon) => make_icon(icon, LARGE_ICON),
        None => Space::new().width(LARGE_ICON).height(LARGE_ICON).into(),
    };
    let label = keytips::label_anchor(
        || keytips::label_id(&keytips::collapsed_anchor(title)),
        text(localized_title.clone())
            .size(PANEL_TITLE_SIZE)
            .align_x(iced::Center)
            .wrapping(if tight {
                iced::advanced::text::Wrapping::Word
            } else {
                iced::advanced::text::Wrapping::None
            }),
    );
    let content = column![icon, label, crate::ui::icons::dropdown_arrow()]
        .align_x(iced::Center)
        .spacing(metrics::GAP);
    let height = if titles {
        metrics::COLLAPSED_H
    } else {
        metrics::COLLAPSED_UNTITLED_H
    };
    let opener = minimize::Opener::Panel(title);
    let face = button(content)
        .on_press(Message::RibbonMinimize(minimize::MinimizeMsg::Click(
            opener,
        )))
        .style(move |theme: &Theme, status| {
            let status = if open && !holds {
                button::Status::Hovered
            } else {
                status
            };
            tool_btn_style(theme, holds, status)
        })
        .height(height)
        .padding(Padding {
            top: metrics::LARGE_PAD_TOP,
            ..Padding::from([0.0, metrics::CONTENT_PAD_SIDE])
        });
    let face = if tight {
        // Narrow: as wide as the title's longest word.
        face.into()
    } else {
        automatic_large_button(localized_title, face.into())
    };
    let face = mouse_area(face)
        .on_enter(Message::RibbonMinimize(minimize::MinimizeMsg::EnterOpener(
            opener,
        )))
        .on_exit(Message::RibbonMinimize(minimize::MinimizeMsg::LeaveOpener(
            opener,
        )));
    keytips::anchor_with(
        || keytips::collapsed_anchor(title),
        PosReport::owned(minimize::opener_id(title), face),
    )
}

impl Default for Ribbon {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::widgets::is_active_tool;
    use super::{CollapseMode, Ribbon, RibbonLayout, StyleContext, panel_measure_key};
    use crate::modules::{IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};
    use rustc_hash::FxHashMap;

    fn measurement_key(tool_id: &'static str, selected_command: &'static str) -> u64 {
        let groups = [RibbonGroup {
            title: "Test",
            tools: vec![RibbonItem::LargeTool(ToolDef {
                id: tool_id,
                label: "Test Tool",
                icon: IconKind::Glyph("T"),
                event: ModuleEvent::Command("TEST".to_string()),
            })],
        }];
        let style_ctx = StyleContext {
            text_style_names: &[],
            active_text_style: "",
            dim_style_names: &[],
            active_dim_style: "",
            mleader_style_names: &[],
            active_mleader_style: "",
            table_style_names: &[],
            active_table_style: "",
        };
        let groups: Vec<_> = groups.iter().collect();
        let mut last_cmd = FxHashMap::default();
        last_cmd.insert("TEST_DROPDOWN", selected_command);
        panel_measure_key(&groups, &last_cmd, &style_ctx, None)
    }

    #[test]
    fn panel_measure_fingerprint_is_stable_and_tracks_panel_structure() {
        assert_eq!(
            measurement_key("TEST", "TEST"),
            measurement_key("TEST", "TEST")
        );
        assert_ne!(
            measurement_key("TEST", "TEST"),
            measurement_key("OTHER", "TEST")
        );
        assert_ne!(
            measurement_key("TEST", "TEST"),
            measurement_key("TEST", "OTHER")
        );
    }

    #[test]
    fn forced_density_views_build_and_keep_collapsed_flyout_content() {
        let mut ribbon = Ribbon::default();
        let first_panel = ribbon.groups_for(ribbon.active).next().unwrap().title;

        for mode in [
            CollapseMode::Full,
            CollapseMode::Compact,
            CollapseMode::Collapsed,
        ] {
            ribbon.set_collapse_mode(mode);
            ribbon.collapsed_open =
                (mode == CollapseMode::Collapsed).then(|| first_panel.to_string());
            let view = ribbon.view(None, false, false, false, 0, 0, false, None);
            std::hint::black_box(view);
        }
    }

    #[test]
    fn block_test_close_panel_extends_draw_only_while_testing() {
        let mut ribbon = Ribbon::default();
        let draw = ribbon
            .modules
            .iter()
            .position(|module| module.id() == "draw")
            .unwrap();
        assert!(
            !ribbon
                .modules
                .iter()
                .any(|module| module.id() == "block_test")
        );
        let ordinary: Vec<_> = ribbon.groups_for(draw).map(|group| group.title).collect();
        assert!(!ordinary.contains(&"Close"));
        ribbon.set_block_editing(true);
        ribbon.set_block_editing(false);
        ribbon.set_block_testing(true);
        assert_eq!(ribbon.active, draw);
        let testing: Vec<_> = ribbon.groups_for(draw).map(|group| group.title).collect();
        assert_eq!(&testing[..ordinary.len()], ordinary.as_slice());
        assert_eq!(testing.last(), Some(&"Close"));
        ribbon.toggle_collapsed_panel("Close");
        ribbon.set_block_testing(false);
        assert!(ribbon.collapsed_open.is_none());
        assert_eq!(
            ribbon
                .groups_for(draw)
                .map(|group| group.title)
                .collect::<Vec<_>>(),
            ordinary
        );
        ribbon.set_block_editing(true);
        assert_eq!(ribbon.modules[ribbon.active].id(), "block_editor");
    }

    #[test]
    fn block_palette_highlight_follows_the_app_state_passed_to_the_view() {
        // The Block Palette button highlight is derived from the app's
        // authoritative `show_block_palette`, threaded into the view at render
        // time: the ribbon keeps no second copy that a BLOCKPALETTE / close
        // handler must remember to keep in sync.
        let ribbon = Ribbon::default();
        let on = ribbon.toggle_state(true);
        assert!(is_active_tool("BLOCKPALETTE", &None, &on));
        let off = ribbon.toggle_state(false);
        assert!(!is_active_tool("BLOCKPALETTE", &None, &off));
    }

    #[test]
    fn a_saved_layout_orders_and_hides_panels_and_tabs() {
        let mut ribbon = Ribbon::default();
        let index = |ribbon: &Ribbon, id: &str| {
            ribbon
                .modules
                .iter()
                .position(|module| module.id() == id)
                .unwrap()
        };
        let draw = index(&ribbon, "draw");
        let insert = index(&ribbon, "insert");
        let titles = |ribbon: &Ribbon, tab: usize| -> Vec<&'static str> {
            ribbon.groups_for(tab).map(|group| group.title).collect()
        };
        let default = titles(&ribbon, draw);
        assert_eq!(&default[..2], ["Draw", "Modify"]);
        assert!(ribbon.is_default_layout());

        let mut layout = RibbonLayout::default();
        layout.panel_order.insert(
            "draw".into(),
            vec!["draw/Modify".into(), "draw/Draw".into()],
        );
        layout.hidden_panels.insert("draw/Block".into());
        layout.hidden_tabs.insert("draw".into());
        ribbon.select(draw);
        ribbon.set_layout(layout);
        assert!(!ribbon.is_default_layout());

        // The hidden tab on show gives way to the first tab still shown.
        assert_ne!(ribbon.active, draw);
        assert!(ribbon.tab_hidden(draw));
        assert!(!ribbon.tab_hidden(insert));

        let arranged = titles(&ribbon, draw);
        assert_eq!(&arranged[..2], ["Modify", "Draw"]);
        assert!(!arranged.contains(&"Block"));
        assert_eq!(arranged.len(), default.len() - 1);
        // Insert's own Block panel is a different panel and stays.
        assert!(titles(&ribbon, insert).contains(&"Block"));
    }

    #[test]
    fn a_drop_saves_the_new_order_and_hidden_panels_keep_their_place() {
        let mut ribbon = Ribbon::default();
        let draw = ribbon
            .modules
            .iter()
            .position(|module| module.id() == "draw")
            .unwrap();
        let titles = |ribbon: &Ribbon| -> Vec<&'static str> {
            ribbon.groups_for(draw).map(|group| group.title).collect()
        };
        let default = titles(&ribbon);
        assert_eq!(&default[..3], ["Draw", "Modify", "Annotation"]);

        // Modify is hidden; Annotation is dropped before Draw.
        ribbon.layout.hidden_panels.insert("draw/Modify".into());
        let mut shown: Vec<String> = titles(&ribbon).iter().map(|t| t.to_string()).collect();
        let annotation = shown.remove(1);
        shown.insert(0, annotation);
        ribbon.reorder_panels("draw", &shown);

        let saved = &ribbon.layout.panel_order["draw"];
        assert_eq!(&saved[..3], ["draw/Annotation", "draw/Modify", "draw/Draw"]);
        assert_eq!(saved.len(), default.len());
        assert_eq!(&titles(&ribbon)[..2], ["Annotation", "Draw"]);

        ribbon.layout.hidden_panels.clear();
        assert_eq!(&titles(&ribbon)[..3], ["Annotation", "Modify", "Draw"]);
    }

    #[test]
    fn escape_returns_from_an_extension_submenu_then_closes_the_panel() {
        let mut ribbon = Ribbon::default();
        ribbon.open_dropdown = Some("DRAWORDER_FRONT".to_string());

        assert!(ribbon.escape_extension());
        assert_eq!(ribbon.open_dropdown.as_deref(), Some("modify_extension"));
        assert!(ribbon.escape_extension());
        assert_eq!(ribbon.open_dropdown, None);
    }
}
