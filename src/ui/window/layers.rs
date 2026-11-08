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
    Description,
    /// How many objects are on the layer, block contents included.
    Objects,
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
            Self::Description => "Description",
            Self::Objects => "Objects",
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
            Self::Transparency => t!("Transp."),
            Self::Description => t!("Description"),
            Self::Objects => t!("Objects"),
        }
    }

    fn width(self) -> f32 {
        match self {
            Self::Color => COL_COLOR,
            Self::Linetype => COL_LT,
            Self::Lineweight => COL_LW,
            Self::Transparency => COL_TRANS,
            Self::Description => COL_DESC,
            Self::Objects => COL_OBJECTS,
            _ => COL_ICON,
        }
    }
}

/// The columns after Name that can be moved and hidden, in their default
/// order.
const COLUMNS: [LayerSortCol; 10] = [
    LayerSortCol::On,
    LayerSortCol::Freeze,
    LayerSortCol::Lock,
    LayerSortCol::Plot,
    LayerSortCol::Color,
    LayerSortCol::Linetype,
    LayerSortCol::Lineweight,
    LayerSortCol::Transparency,
    LayerSortCol::Objects,
    LayerSortCol::Description,
];

/// The columns a narrow pallet keeps (#2610, under 420 px): what is changed
/// most, the rest stays reachable by widening the pallet.
const NARROW_COLUMNS: [LayerSortCol; 4] = [
    LayerSortCol::On,
    LayerSortCol::Freeze,
    LayerSortCol::Lock,
    LayerSortCol::Color,
];

/// The pallet's width band: wide shows labelled toolbar buttons, medium and
/// narrow icon buttons with tooltips, narrow also only the main columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WidthBand {
    Wide,
    Medium,
    Narrow,
}

impl WidthBand {
    pub fn of(width: f32) -> Self {
        if width >= 760.0 {
            Self::Wide
        } else if width >= 420.0 {
            Self::Medium
        } else {
            Self::Narrow
        }
    }
}

/// The columns shown, in order, for the pallet's width.
fn shown_columns(band: WidthBand) -> Vec<LayerSortCol> {
    column_layout()
        .into_iter()
        .filter(|(col, shown)| {
            *shown && (band != WidthBand::Narrow || NARROW_COLUMNS.contains(col))
        })
        .map(|(col, _)| col)
        .collect()
}

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

/// A node of the Layer manager's filter tree: which layers the table shows
/// (#2610).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TreeNode {
    #[default]
    All,
    /// Layers with objects on them.
    Used,
    /// Layers without objects.
    Unused,
    /// Layers of the objects selected in the drawing.
    Selected,
    /// The layers of one xref (`name|layer`).
    Xref(String),
    /// Layers with a colour or freeze override in a layout viewport.
    ViewportOverrides,
    Group(String),
    /// A LAYERFILTER filter.
    Filter(String),
    /// The layers of what FILTER found, sent over from its result.
    Found,
}

impl TreeNode {
    pub fn label(&self) -> String {
        match self {
            Self::All => crate::tr!("layer-groups", "all-layers"),
            Self::Used => t!("Used layers").into_owned(),
            Self::Unused => t!("Unused layers").into_owned(),
            Self::Selected => t!("Layers of selected objects").into_owned(),
            Self::Xref(name) | Self::Group(name) | Self::Filter(name) => name.clone(),
            Self::ViewportOverrides => t!("Viewport overrides").into_owned(),
            Self::Found => t!("Layers of the filter result").into_owned(),
        }
    }
}

/// A part of the filter tree that opens and closes (#2610).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeSection {
    /// "All" and everything under it.
    All,
    Groups,
    /// The property (LAYERFILTER) filters.
    Filters,
    Xrefs,
}

impl TreeSection {
    pub fn label(self) -> String {
        match self {
            Self::All => crate::tr!("layer-groups", "all-layers"),
            Self::Groups => t!("Groups").into_owned(),
            Self::Filters => t!("Property filters").into_owned(),
            Self::Xrefs => t!("Xrefs").into_owned(),
        }
    }
}

/// A line of the filter tree: a section heading or a node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeRow {
    Section(TreeSection, u8),
    Node(TreeNode, u8),
}

/// What a filter tree node's right-click menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeAction {
    On,
    Off,
    Freeze,
    Thaw,
    Lock,
    Unlock,
    /// A LAYERFILTER filter: open it in FILTER's editor.
    Edit,
    /// A LAYERFILTER filter or a group: delete it; its layers stay.
    Delete,
    /// A group or a filter: rename it in the tree.
    Rename,
    /// "Unused layers": the Purge window with them ticked.
    Purge,
}

/// What a layer row's right-click menu does. The menu has one level: a
/// choice with options lists them under it, inside the menu (#2610).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerMenuAction {
    SetCurrent,
    New,
    Rename,
    Delete,
    /// Select the drawing's objects on the selected layers.
    SelectObjects,
    NewGroup,
    /// Add the selected layers to the group, or take them out when they
    /// are all in it already (the item shows a tick then).
    AddToGroup(String),
    RemoveFromGroup,
    /// FILTER with this layer (the selected ones) as its condition.
    FilterObjects,
    /// Merge the other selected layers into this one (LAYMRG).
    MergeHere,
}

/// Layers the Layer manager never deletes: "0", Defpoints, the current layer
/// and a layer that belongs to an xref (`XREF|name`).
pub fn layer_is_protected(name: &str, current: &str) -> bool {
    name == "0"
        || name.eq_ignore_ascii_case("Defpoints")
        || name.eq_ignore_ascii_case(current)
        || name.contains('|')
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
    pub description: String,
    /// Objects on the layer, and how many of them are inside blocks.
    pub objects: usize,
    pub in_blocks: usize,
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
            description: String::new(),
            objects: 0,
            in_blocks: 0,
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
    /// Transparency being typed: (layer name, text). Enter or leaving the
    /// field applies it as one undo step; Esc drops it.
    pub transparency_edit: Option<(String, String)>,
    /// Description being typed: (layer name, text), applied like the
    /// transparency.
    pub description_edit: Option<(String, String)>,
    /// Objects per layer (lower-case name → total, inside blocks), and what
    /// they were counted for, so they are counted again only after a change.
    pub counts: std::collections::HashMap<String, (usize, usize)>,
    pub counts_key: Option<(u64, u64)>,
    /// What the filter tree was last worked out from: the selection, the
    /// layer table and the filters.
    pub tree_key: Option<u64>,
    /// The last click was in the table, so ↑/↓, Ctrl+A, Del, F2 and Esc act
    /// on its rows. A click in the drawing or typing a command ends it.
    pub keys_active: bool,
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
    /// The filter tree's node shown in the table.
    pub view: TreeNode,
    /// Show the layers the node leaves out instead.
    pub invert: bool,
    /// Layers (lower case) of the objects selected in the drawing.
    pub selected_object_layers: Vec<String>,
    /// LAYERFILTER filters with the layers each one matches.
    pub filters: Vec<(String, Vec<String>)>,
    /// How deep each of `filters` sits: 0 at the top, 1 under another…
    pub filter_depths: Vec<u8>,
    /// The layers of FILTER's last result sent here (the Found node).
    pub found_layers: Vec<String>,
    /// The layers whose eye is on: their objects are highlighted in the
    /// drawing, the selection kept.
    pub eye_layers: Vec<String>,
    /// How many objects the eyes light up.
    pub highlighted: usize,
    /// What the highlight was made for: geometry epoch and layer names.
    pub highlight_key: Option<(u64, Vec<String>)>,
    /// The tree's sections the user closed.
    pub closed_sections: Vec<TreeSection>,
    /// A group or filter being renamed in the tree, with the typed name.
    pub tree_rename: Option<(TreeNode, String)>,
    /// The last click was in the tree: F2 renames the node shown.
    pub tree_focus: bool,
    /// Rows pressed and possibly being dragged onto a group.
    pub dragging: bool,
    /// The group the dragged rows are over.
    pub drag_over: Option<String>,
    /// The compact toolbar's "…" menu is open.
    pub more_menu: bool,
    /// The compact toolbar's search field is open.
    pub search_open: bool,
    /// The ribbon's layer list follows the chosen node (mirrors the app's
    /// setting for the footer's box).
    pub ribbon_follows: bool,
}

impl Default for LayerPanel {
    fn default() -> Self {
        Self {
            layers: vec![Layer::new("0", AcadColor::Index(7))],
            selected: None,
            selected_multi: Vec::new(),
            editing: None,
            edit_buf: String::new(),
            transparency_edit: None,
            description_edit: None,
            counts: Default::default(),
            counts_key: None,
            tree_key: None,
            keys_active: false,
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
            view: TreeNode::All,
            invert: false,
            selected_object_layers: Vec::new(),
            filters: Vec::new(),
            found_layers: Vec::new(),
            eye_layers: Vec::new(),
            highlighted: 0,
            highlight_key: None,
            filter_depths: Vec::new(),
            closed_sections: Vec::new(),
            tree_rename: None,
            tree_focus: false,
            dragging: false,
            drag_over: None,
            more_menu: false,
            search_open: false,
            ribbon_follows: false,
        }
    }
}

impl LayerPanel {
    /// Take the drawing's groups; a shown group that is gone shows all.
    pub fn set_groups(&mut self, groups: Vec<(String, Vec<String>)>) {
        if let Some(shown) = self.shown_group() {
            if !groups.iter().any(|(name, _)| name == shown) {
                self.view = TreeNode::All;
            }
        }
        self.groups = groups;
    }

    /// The group shown, if a group node is.
    pub fn shown_group(&self) -> Option<&String> {
        match &self.view {
            TreeNode::Group(name) => Some(name),
            _ => None,
        }
    }

    /// Show `group`, or every layer for `None`.
    pub fn set_group(&mut self, group: Option<String>) {
        self.view = group.map_or(TreeNode::All, TreeNode::Group);
    }

    /// The xrefs whose layers are in the drawing (`name|layer`).
    fn xref_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .layers
            .iter()
            .filter_map(|layer| layer.name.split_once('|').map(|(xref, _)| xref.to_string()))
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        names
    }

    /// The tree's nodes in order, with how deep each sits.
    pub fn tree_nodes(&self) -> Vec<(TreeNode, u8)> {
        let mut nodes = vec![
            (TreeNode::All, 0),
            (TreeNode::Used, 1),
            (TreeNode::Unused, 1),
            (TreeNode::Selected, 1),
        ];
        nodes.extend(
            self.xref_names()
                .into_iter()
                .map(|x| (TreeNode::Xref(x), 1)),
        );
        if !self.vp_cols.is_empty() {
            nodes.push((TreeNode::ViewportOverrides, 1));
        }
        nodes.extend(
            self.groups
                .iter()
                .map(|(g, _)| (TreeNode::Group(g.clone()), 1)),
        );
        nodes.extend(
            self.filters
                .iter()
                .map(|(f, _)| (TreeNode::Filter(f.clone()), 1)),
        );
        if !self.found_layers.is_empty() {
            nodes.push((TreeNode::Found, 1));
        }
        nodes
    }

    /// The layers `node` holds, by name.
    pub fn node_layers(&self, node: &TreeNode) -> Vec<String> {
        self.layers
            .iter()
            .filter(|layer| self.in_node(node, layer))
            .map(|layer| layer.name.clone())
            .collect()
    }

    /// Whether `layer` is in the node chosen in the tree (inverted when the
    /// tree is): what the ribbon's list follows with LM-25 on.
    pub fn in_chosen_node(&self, layer: &Layer) -> bool {
        self.in_view(layer) != self.invert
    }

    /// Whether `layer` belongs to the node shown.
    fn in_view(&self, layer: &Layer) -> bool {
        self.in_node(&self.view, layer)
    }

    fn in_node(&self, node: &TreeNode, layer: &Layer) -> bool {
        let named = |list: &[String]| {
            list.iter()
                .any(|name| name.eq_ignore_ascii_case(&layer.name))
        };
        match node {
            TreeNode::All => true,
            TreeNode::Used => layer.objects > 0,
            TreeNode::Unused => layer.objects == 0,
            TreeNode::Selected => self
                .selected_object_layers
                .contains(&layer.name.to_lowercase()),
            TreeNode::Xref(xref) => layer
                .name
                .split_once('|')
                .is_some_and(|(owner, _)| owner.eq_ignore_ascii_case(xref)),
            TreeNode::ViewportOverrides => {
                layer.vp_frozen.iter().any(|frozen| *frozen)
                    || layer.vp_colors.iter().any(Option::is_some)
            }
            TreeNode::Group(group) => self
                .groups
                .iter()
                .find(|(name, _)| name == group)
                .is_some_and(|(_, members)| named(members)),
            TreeNode::Found => named(&self.found_layers),
            TreeNode::Filter(filter) => self
                .filters
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(filter))
                .is_some_and(|(_, members)| named(members)),
        }
    }

    /// Whether the layer's eye is on.
    pub fn eye_on(&self, name: &str) -> bool {
        self.eye_layers.iter().any(|l| l.eq_ignore_ascii_case(name))
    }

    /// Opens or closes a section of the tree.
    pub fn toggle_section(&mut self, section: TreeSection) {
        if let Some(at) = self.closed_sections.iter().position(|s| *s == section) {
            self.closed_sections.remove(at);
        } else {
            self.closed_sections.push(section);
        }
    }

    fn section_open(&self, section: TreeSection) -> bool {
        !self.closed_sections.contains(&section)
    }

    /// The tree's lines as drawn: "All" with the built-in nodes under it,
    /// then Groups, Property filters and Xrefs, each opening and closing.
    pub fn tree_rows(&self) -> Vec<TreeRow> {
        let mut rows = vec![TreeRow::Section(TreeSection::All, 0)];
        if !self.section_open(TreeSection::All) {
            return rows;
        }
        rows.extend(
            [TreeNode::Used, TreeNode::Unused, TreeNode::Selected]
                .into_iter()
                .map(|node| TreeRow::Node(node, 1)),
        );
        let section = |rows: &mut Vec<TreeRow>, section: TreeSection, nodes: Vec<TreeNode>| {
            rows.push(TreeRow::Section(section, 1));
            if self.section_open(section) {
                for node in nodes {
                    // A filter under another sits one deeper (#2610).
                    let extra = match &node {
                        TreeNode::Filter(name) => self
                            .filters
                            .iter()
                            .position(|(f, _)| f == name)
                            .and_then(|at| self.filter_depths.get(at).copied())
                            .unwrap_or(0),
                        _ => 0,
                    };
                    rows.push(TreeRow::Node(node, 2 + extra));
                }
            }
        };
        section(
            &mut rows,
            TreeSection::Groups,
            self.groups
                .iter()
                .map(|(g, _)| TreeNode::Group(g.clone()))
                .collect(),
        );
        section(
            &mut rows,
            TreeSection::Filters,
            self.filters
                .iter()
                .map(|(f, _)| TreeNode::Filter(f.clone()))
                .collect(),
        );
        let xrefs = self.xref_names();
        if !xrefs.is_empty() {
            section(
                &mut rows,
                TreeSection::Xrefs,
                xrefs.into_iter().map(TreeNode::Xref).collect(),
            );
        }
        if !self.vp_cols.is_empty() {
            rows.push(TreeRow::Node(TreeNode::ViewportOverrides, 1));
        }
        if !self.found_layers.is_empty() {
            rows.push(TreeRow::Node(TreeNode::Found, 1));
        }
        rows
    }

    /// How many layers a node holds.
    pub fn node_count(&self, node: &TreeNode) -> usize {
        self.layers
            .iter()
            .filter(|layer| self.in_node(node, layer))
            .count()
    }

    /// The filter tree beside the table on a wide pallet: flat lines, the
    /// chosen one in the selection colour, each with its layer count.
    fn tree(&self) -> Element<'_, Message> {
        let mut list = column![].spacing(0);
        for line in self.tree_rows() {
            let (depth, toggle, icon, label, count, chosen, on_press) = match &line {
                TreeRow::Section(section, depth) => {
                    let (count, chosen, on_press) = if *section == TreeSection::All {
                        (
                            Some(self.layers.len()),
                            self.view == TreeNode::All,
                            Message::LayerShow(TreeNode::All),
                        )
                    } else {
                        (None, false, Message::LayerTreeSection(*section))
                    };
                    let icon = match section {
                        TreeSection::Groups => Some(crate::ui::icons::FOLDER_OPEN),
                        TreeSection::Filters => Some(crate::ui::icons::ST_FILTER),
                        TreeSection::Xrefs => Some(crate::ui::icons::DOC),
                        TreeSection::All => None,
                    };
                    (
                        *depth,
                        Some(*section),
                        icon,
                        section.label(),
                        count,
                        chosen,
                        on_press,
                    )
                }
                TreeRow::Node(node, depth) => {
                    let icon = match node {
                        TreeNode::Group(_) => Some(crate::ui::icons::FOLDER_OPEN),
                        TreeNode::Filter(_) => Some(crate::ui::icons::ST_FILTER),
                        TreeNode::Xref(_) => Some(crate::ui::icons::DOC),
                        _ => None,
                    };
                    (
                        *depth,
                        None,
                        icon,
                        node.label(),
                        Some(self.node_count(node)),
                        self.view == *node,
                        Message::LayerShow(node.clone()),
                    )
                }
            };
            // The open/close arrow, a button of its own: on "All" the label
            // shows the node.
            let arrow: Element<'_, Message> = match toggle {
                Some(section) => button(if self.section_open(section) {
                    crate::ui::icons::themed_arrow_down(8.0)
                } else {
                    crate::ui::icons::themed_arrow_right(8.0)
                })
                .on_press(Message::LayerTreeSection(section))
                .style(button::text)
                .padding(0)
                .width(12)
                .into(),
                None => iced::widget::Space::new().width(12).into(),
            };
            let icon: Element<'_, Message> = match icon {
                Some(bytes) => container(crate::ui::icons::themed_secondary(bytes, 12.0))
                    .width(16)
                    .into(),
                None => iced::widget::Space::new().width(0).into(),
            };
            let no_selection = matches!(&line, TreeRow::Node(TreeNode::Selected, _))
                && self.selected_object_layers.is_empty();
            let count: Element<'_, Message> = match count {
                _ if no_selection => text("—").size(FONT_SZ).style(muted_style).into(),
                Some(n) => text(n.to_string())
                    .size(FONT_SZ)
                    .style(move |theme: &Theme| text::Style {
                        color: Some(if chosen {
                            selection_colors(theme).1
                        } else {
                            theme.palette().background.base.text.scale_alpha(0.6)
                        }),
                    })
                    .into(),
                None => iced::widget::Space::new().width(0).into(),
            };
            let label = text(label)
                .size(FONT_SZ)
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Fill);
            let label = if no_selection {
                label.style(muted_style)
            } else {
                label
            };
            let entry = button(
                row![
                    label,
                    count
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(on_press)
            .style(move |theme: &Theme, status| tree_line_style(theme, status, chosen))
            .padding([3, 4])
            .width(Fill);
            let entry: Element<'_, Message> = match &line {
                // Renaming in place: a field instead of the line.
                TreeRow::Node(node, _)
                    if self.tree_rename.as_ref().is_some_and(|(n, _)| n == node) =>
                {
                    let typed = self.tree_rename.as_ref().map_or("", |(_, t)| t.as_str());
                    text_input("", typed)
                        .id(tree_rename_input_id())
                        .on_input(Message::LayerTreeRenameEdit)
                        .on_submit(Message::LayerTreeRenameCommit)
                        .size(FONT_SZ)
                        .padding([2, 4])
                        .width(Fill)
                        .style(|theme, status| field_style_with_radius(theme, status, 2.0))
                        .into()
                }
                TreeRow::Node(node, _) => {
                    let menu_node = node.clone();
                    let entry = iced_aw::ContextMenu::new(entry, move || tree_menu(&menu_node));
                    let mut area = mouse_area(entry);
                    if let TreeNode::Group(group) = node {
                        // Dragged rows drop onto a group (#2610).
                        area = area
                            .on_enter(Message::LayerDragOver(Some(group.clone())))
                            .on_exit(Message::LayerDragOver(None))
                            .on_release(Message::LayerDrop(group.clone()));
                    }
                    if matches!(node, TreeNode::Group(_) | TreeNode::Filter(_)) {
                        area = area.on_double_click(Message::LayerTreeAction(
                            node.clone(),
                            TreeAction::Rename,
                        ));
                    }
                    let dropping = matches!(node, TreeNode::Group(g)
                        if self.dragging && self.drag_over.as_deref() == Some(g.as_str()));
                    container(area)
                        .style(move |theme: &Theme| container::Style {
                            border: Border {
                                color: if dropping {
                                    theme.palette().primary.base.color
                                } else {
                                    Color::TRANSPARENT
                                },
                                width: 1.0,
                                radius: 3.0.into(),
                            },
                            ..Default::default()
                        })
                        .into()
                }
                TreeRow::Section(..) => entry.into(),
            };
            // The node's eye: all its layers' objects lit in the drawing.
            let eye: Element<'_, Message> = match &line {
                TreeRow::Node(node, _) if self.tree_rename.is_none() => {
                    let members = self.node_layers(node);
                    let on = !members.is_empty() && members.iter().all(|m| self.eye_on(m));
                    let eye = button(if on {
                        crate::ui::icons::themed_primary(crate::ui::icons::EYE, 11.0)
                    } else {
                        crate::ui::icons::themed_secondary(crate::ui::icons::EYE, 11.0)
                    })
                    .on_press(Message::LayerNodeEye(node.clone()))
                    .style(button::text)
                    .padding([2, 2]);
                    // Shown while on, or under the pointer: the tree stays
                    // quiet as in the mockup.
                    if on {
                        eye.into()
                    } else {
                        iced::widget::hover(
                            iced::widget::Space::new().width(15).height(15),
                            eye,
                        )
                    }
                }
                _ => iced::widget::Space::new().width(15).into(),
            };
            list = list.push(
                row![
                    iced::widget::Space::new().width(f32::from(depth) * 14.0),
                    arrow,
                    icon,
                    entry,
                    eye
                ]
                .spacing(2)
                .align_y(iced::Center),
            );
        }
        container(scrollable(list).height(Length::Fill))
            .width(Length::Fixed(TREE_W))
            .height(Length::Fill)
            .padding([6, 4])
            .style(|theme: &Theme| container::Style {
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    /// Under the table: invert the node, what the eyes light, the current
    /// layer and the node shown with its counts.
    fn tree_footer(&self) -> Element<'_, Message> {
        let ribbon_follows = self.ribbon_follows;
        let shown = self.visible_indices().len();
        let selected = self.selected_names().len();
        let node = match &self.view {
            TreeNode::Group(name) => crate::tf!("Group “{}”", name).into_owned(),
            TreeNode::Filter(name) => crate::tf!("Filter “{}”", name).into_owned(),
            other => other.label(),
        };
        let mut summary = format!(
            "{} · {}",
            node,
            crate::tf!("{} of {} layers shown", shown, self.layers.len())
        );
        if selected > 0 {
            summary.push_str(" · ");
            summary.push_str(&crate::tf!("{} selected", selected));
        }
        let lit: Element<'_, Message> = if self.eye_layers.is_empty() {
            iced::widget::Space::new().width(0).into()
        } else {
            row![
                crate::ui::icons::themed(crate::ui::icons::EYE, 12.0),
                text(crate::tf!(
                    "Highlighting {} objects of {}. Selection unchanged.",
                    self.highlighted,
                    self.eye_layers.join(", ")
                ))
                .size(FONT_SZ),
            ]
            .spacing(6)
            .align_y(iced::Center)
            .into()
        };
        container(
            row![
                checkbox(self.invert)
                    .label(t!("Invert filter"))
                    .text_size(FONT_SZ)
                    .size(13)
                    .on_toggle(Message::LayerInvert),
                lit,
                iced::widget::Space::new().width(Length::Fill),
                checkbox(ribbon_follows)
                    .label(t!("Ribbon list follows the tree"))
                    .text_size(FONT_SZ)
                    .size(13)
                    .on_toggle(Message::LayerRibbonFollows),
                text(format!("{}:", t!("Current layer")))
                    .size(FONT_SZ)
                    .style(muted_style),
                text(self.current_layer.as_str()).size(FONT_SZ),
                text(summary).size(FONT_SZ).style(muted_style),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .padding([3, 8])
        .width(Length::Fill)
        .into()
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
                    description: l.description.clone(),
                    objects: 0,
                    in_blocks: 0,
                    vp_frozen,
                    vp_colors,
                }
            })
            .collect();

        self.apply_counts();
        // Re-resolve the selection against the rebuilt rows by name.
        self.selected = anchor_name.and_then(|n| self.layers.iter().position(|l| l.name == n));
        self.selected_multi = multi_names
            .iter()
            .filter_map(|n| self.layers.iter().position(|l| l.name == *n))
            .collect();

        self.apply_sort();
    }

    /// Take the drawing's object counts per layer (see `counts`).
    pub fn set_counts(
        &mut self,
        counts: std::collections::HashMap<String, (usize, usize)>,
        key: (u64, u64),
    ) {
        self.counts = counts;
        self.counts_key = Some(key);
        self.apply_counts();
        if self.sort_col == Some(LayerSortCol::Objects) {
            self.apply_sort();
        }
    }

    fn apply_counts(&mut self) {
        for layer in &mut self.layers {
            let (objects, in_blocks) = self
                .counts
                .get(&layer.name.to_lowercase())
                .copied()
                .unwrap_or_default();
            layer.objects = objects;
            layer.in_blocks = in_blocks;
        }
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
                LayerSortCol::Description => a
                    .description
                    .to_lowercase()
                    .cmp(&b.description.to_lowercase()),
                LayerSortCol::Objects => a.objects.cmp(&b.objects),
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
        self.layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| self.in_view(layer) != self.invert)
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
        let band = WidthBand::of(inner_w);
        let compact = band != WidthBand::Wide;
        let columns = shown_columns(band);
        // Delete is off when every selected layer is one that can't go;
        // Set Current is off for the current layer and a frozen one (#2610).
        let can_delete = self
            .selected_names()
            .iter()
            .any(|name| !layer_is_protected(name, &self.current_layer));
        let can_set_current = self
            .selected
            .and_then(|i| self.layers.get(i))
            .is_some_and(|layer| layer.name != self.current_layer && !layer.frozen);

        // In a layout with viewports, a new layer can start frozen in all of
        // them, so it shows only in the viewports that thaw it.
        let new_vp_frozen: Element<'_, Message> = if self.vp_cols.is_empty() {
            iced::widget::Space::new().width(0).into()
        } else {
            toolbar_btn(
                crate::ui::icons::PLUS,
                t!("New, frozen in every viewport"),
                Message::LayerNewVpFrozen,
                compact,
            )
        };

        let has_selection = !self.selected_names().is_empty();
        // Without room for the tree beside the table, its nodes are a list
        // opening below (#2610).
        let node_picker: Element<'_, Message> = if band == WidthBand::Wide {
            iced::widget::Space::new().width(0).into()
        } else {
            let nodes: Vec<TreeNode> = self
                .tree_nodes()
                .into_iter()
                .map(|(node, _)| node)
                .collect();
            crate::ui::dropdown_list::pick_list(Some(self.view.clone()), nodes, TreeNode::label)
                .on_select(Message::LayerShow)
                .text_size(FONT_SZ)
                .padding([3, 6])
                .width(Length::Fixed(130.0))
                .into()
        };

        // ── Toolbar ───────────────────────────────────────────────────────
        let toolbar: Element<'_, Message> = if compact {
            self.compact_toolbar(can_delete, can_set_current)
        } else {
            self.wide_toolbar(can_delete, can_set_current, has_selection, new_vp_frozen, node_picker)
        };
        let _ = has_selection;

        // The room the table has: beside the tree on a wide pallet.
        let table_w = if band == WidthBand::Wide && !self.columns_menu {
            (inner_w - TREE_W - 2.0).max(0.0)
        } else {
            inner_w
        };

        // ── Column header ─────────────────────────────────────────────────
        let sc = self.sort_col;
        let sa = self.sort_asc;
        let mut header_row = row![
            text(t!("Status")).size(10).style(muted_style).width(COL_STATUS),
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
        for col in &columns {
            header_row = header_row.push(sortable_header(
                col.label(),
                *col,
                Length::Fixed(col.width()),
                sc,
                sa,
            ));
        }

        for vp in self.vp_cols.iter().filter(|vp| active_vp.is_none_or(|h| h == vp.handle)) {
            header_row = header_row.push(
                text(vp.label.as_str())
                    .size(10)
                    .style(muted_style)
                    .width(Length::Fixed(COL_ICON * 2.0 + 4.0)),
            );
        }
        // Over the rows' eye buttons, after the same filler the rows have.
        header_row = header_row
            .width(Fill)
            .push(iced::widget::Space::new().width(Fill))
            .push(iced::widget::Space::new().width(COL_EYE));

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
            let desc_buf = self
                .description_edit
                .as_ref()
                .filter(|(name, _)| *name == layer.name)
                .map(|(_, typed)| typed.as_str());
            let trans_buf = self
                .transparency_edit
                .as_ref()
                .filter(|(name, _)| *name == layer.name)
                .map(|(_, typed)| typed.as_str());

            let (ltc, lwc) = if is_anchor {
                (Some(&self.linetype_combo), Some(&self.lw_combo))
            } else {
                (None, None)
            };

            let row = layer_row(
                i,
                layer,
                is_sel,
                is_current,
                is_editing,
                &self.edit_buf,
                color_open,
                trans_buf,
                desc_buf,
                ltc,
                lwc,
                &self.vp_cols,
                active_vp,
                name_col_w,
                &columns,
                self.eye_on(&layer.name),
            );
            let (name, groups, in_group) = (
                layer.name.clone(),
                self.groups
                    .iter()
                    .map(|(g, members)| {
                        let ticked = members.iter().any(|m| m.eq_ignore_ascii_case(&layer.name));
                        (g.clone(), ticked)
                    })
                    .collect::<Vec<_>>(),
                self.shown_group().is_some(),
            );
            rows_col = rows_col.push(iced_aw::ContextMenu::new(row, move || {
                row_menu(&name, &groups, in_group)
            }));
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
                .width(Length::Fixed(table_w))
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
            column![toolbar, column_chooser(), grid]
                .height(Length::Fill)
                .into()
        } else if band == WidthBand::Wide {
            // A wide pallet has the tree beside the table.
            column![
                toolbar,
                row![self.tree(), grid].height(Length::Fill),
                self.tree_footer(),
            ]
            .height(Length::Fill)
            .into()
        } else {
            column![toolbar, grid, self.compact_footer()]
                .height(Length::Fill)
                .into()
        }
    }

    /// A wide pallet's toolbar: labelled buttons as in the mockup.
    fn wide_toolbar<'a>(
        &'a self,
        can_delete: bool,
        can_set_current: bool,
        has_selection: bool,
        new_vp_frozen: Element<'a, Message>,
        node_picker: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let compact = false;
        container(
            row![
                toolbar_btn(
                    crate::ui::icons::PLUS,
                    t!("New"),
                    Message::LayerNew,
                    compact
                ),
                new_vp_frozen,
                toolbar_btn_cond(
                    crate::ui::icons::TRASH,
                    t!("Delete"),
                    Message::LayerDelete,
                    can_delete,
                    compact,
                ),
                toolbar_btn_cond(
                    crate::ui::icons::CHECK,
                    t!("Set Current"),
                    Message::LayerSetCurrent,
                    can_set_current,
                    compact,
                ),
                toolbar_divider(),
                // A property filter, made with FILTER's rows.
                toolbar_btn(
                    crate::ui::icons::ST_FILTER,
                    t!("New Filter"),
                    Message::LayerFilterEdit(Some(String::new())),
                    compact,
                ),
                // A group from the selected layers.
                toolbar_btn_cond(
                    crate::ui::icons::FOLDER_OPEN,
                    crate::tr!("layer-groups", "new-group").into(),
                    Message::LayerGroupNew,
                    has_selection,
                    compact,
                ),
                toolbar_divider(),
                toolbar_btn(
                    crate::ui::icons::LIST,
                    t!("Layer States"),
                    Message::LayerMoreRun(Box::new(Message::Command("LAYERSTATE".into()))),
                    compact,
                ),
                // LAYERP: the last layer-property change undone (#2610).
                toolbar_btn(
                    crate::ui::icons::UNDO,
                    t!("Previous"),
                    Message::Command("LAYERP".into()),
                    compact,
                ),
                // Layers from another drawing (#2610).
                toolbar_btn(
                    crate::ui::icons::IMPORT,
                    t!("Import..."),
                    Message::LayerMoreRun(Box::new(Message::LayerImport(
                        crate::app::layer_import::LayerImportMsg::Pick,
                    ))),
                    compact,
                ),
                node_picker,
                iced::widget::Space::new().width(Length::Fill),
                // Search box: filters rows by name as the user types (#343).
                text_input(t!("Search…").as_ref(), &self.filter)
                    .on_input(Message::LayerManagerFilterChanged)
                    .size(FONT_SZ)
                    .padding([3, 6])
                    .width(Length::Fixed(if compact { 120.0 } else { 170.0 }))
                    .style(|theme, status| field_style_with_radius(theme, status, 2.0)),
                // The search text as a filter, edited with FILTER's rows.
                toolbar_text_btn(t!("Advanced…"), Message::LayerFilterEdit(None)),
                // The search text kept as a property filter (LM-22a).
                if self.filter.trim().is_empty() {
                    Element::from(iced::widget::Space::new().width(0))
                } else {
                    toolbar_text_btn(t!("Save as filter"), Message::LayerSaveSearch)
                },
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        })
        .width(Length::Fill)
        .padding([4, 8])
        .into()
    }

    /// A medium or narrow pallet's top (#2610 mockup, Narrow and Docked):
    /// New, Delete, Set Current, Previous and a "…" menu opening below with
    /// the rest; a search icon that opens the field; under them the tree as
    /// a list with the node's count.
    fn compact_toolbar(&self, can_delete: bool, can_set_current: bool) -> Element<'_, Message> {
        let more_items: Vec<Element<'_, Message>> = vec![
            more_item(t!("New Filter"), Message::LayerFilterEdit(Some(String::new()))),
            more_item(
                crate::tr!("layer-groups", "new-group").into(),
                Message::LayerGroupNew,
            ),
            more_item(t!("Layer States"), Message::Command("LAYERSTATE".into())),
            more_item(
                t!("Import..."),
                Message::LayerImport(crate::app::layer_import::LayerImportMsg::Pick),
            ),
            more_item(t!("Advanced…"), Message::LayerFilterEdit(None)),
        ];
        let more_head = button(crate::ui::icons::themed(crate::ui::icons::MORE, 12.0))
            .on_press(Message::LayerMoreMenu(!self.more_menu))
            .style(crate::ui::style::form::button_style(self.more_menu))
            .padding([4, 6]);
        // The overlay takes the "…" button's width unless it is given one,
        // and iced_aw's BottomStart opens it beside the button, not below.
        let more = iced_aw::DropDown::new(
            more_head,
            container(iced::widget::Column::with_children(more_items).width(Length::Fill))
                .padding([4, 0])
                .style(crate::ui::color_select::popup_panel_style),
            self.more_menu,
        )
        .width(Length::Fixed(MORE_MENU_W))
        .alignment(iced_aw::drop_down::Alignment::Bottom)
        .offset(2.0)
        .on_dismiss(Message::LayerMoreMenu(false));
        let search_btn = button(crate::ui::icons::themed(crate::ui::icons::SEARCH, 12.0))
            .on_press(Message::LayerSearchOpen(!self.search_open))
            .style(crate::ui::style::form::button_style(self.search_open))
            .padding([4, 6]);
        let tools = row![
            toolbar_btn(crate::ui::icons::PLUS, t!("New"), Message::LayerNew, true),
            toolbar_btn_cond(crate::ui::icons::TRASH, t!("Delete"), Message::LayerDelete, can_delete, true),
            toolbar_btn_cond(
                crate::ui::icons::CHECK,
                t!("Set Current"),
                Message::LayerSetCurrent,
                can_set_current,
                true,
            ),
            toolbar_btn(
                crate::ui::icons::UNDO,
                t!("Previous"),
                Message::Command("LAYERP".into()),
                true,
            ),
            more,
            iced::widget::Space::new().width(Length::Fill),
            with_tip(search_btn, t!("Search…"), true),
        ]
        .spacing(3)
        .align_y(iced::Center);
        // The tree's nodes as a list opening below, each with its count.
        let nodes: Vec<TreeNode> = self.tree_nodes().into_iter().map(|(n, _)| n).collect();
        let counts: Vec<(TreeNode, usize)> =
            nodes.iter().map(|n| (n.clone(), self.node_count(n))).collect();
        let picker = crate::ui::dropdown_list::pick_list(Some(self.view.clone()), nodes, move |n: &TreeNode| {
            let count = counts.iter().find(|(c, _)| c == n).map_or(0, |(_, k)| *k);
            format!("{}  ·  {count}", n.label())
        })
        .on_select(Message::LayerShow)
        .text_size(FONT_SZ)
        .padding([3, 6])
        .width(Length::Fill);
        let mut top = column![tools, picker].spacing(4);
        if self.search_open || !self.filter.is_empty() {
            top = top.push(
                row![
                    text_input(t!("Search…").as_ref(), &self.filter)
                        .on_input(Message::LayerManagerFilterChanged)
                        .size(FONT_SZ)
                        .padding([3, 6])
                        .width(Length::Fill)
                        .style(|theme, status| field_style_with_radius(theme, status, 2.0)),
                    toolbar_text_btn(t!("Advanced…"), Message::LayerFilterEdit(None)),
                ]
                .spacing(4)
                .align_y(iced::Center),
            );
        }
        container(top)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(theme.palette().background.weak.color)),
                ..Default::default()
            })
            .width(Length::Fill)
            .padding([4, 6])
            .into()
    }

    /// A compact pallet's footer, one line: the current layer and how many
    /// layers the node shows.
    fn compact_footer(&self) -> Element<'_, Message> {
        container(
            row![
                text(format!("{}:", t!("Current layer")))
                    .size(FONT_SZ)
                    .style(muted_style),
                text(self.current_layer.as_str())
                    .size(FONT_SZ)
                    .wrapping(iced::widget::text::Wrapping::None),
                text(format!(
                    "· {}",
                    crate::tf!("{} of {} layers shown", self.visible_indices().len(), self.layers.len())
                ))
                .size(FONT_SZ)
                .style(muted_style)
                .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .clip(true)
        .padding([4, 8])
        .width(Length::Fill)
        .into()
    }
}

/// A line of the compact toolbar's "…" menu: runs its message and closes.
fn more_item<'a>(label: Cow<'static, str>, msg: Message) -> Element<'a, Message> {
    menu_item(label, Message::LayerMoreRun(Box::new(msg)), 0.0, None, false)
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
    let mut content = row![text(label)
        .size(10)
        .style(muted_style)
        .wrapping(iced::widget::text::Wrapping::None)]
    .spacing(3)
        .align_y(iced::Center);
    if active == Some(col) {
        content = content.push(if asc {
            crate::ui::icons::themed_arrow_up(8.0)
        } else {
            crate::ui::icons::themed_arrow_down(8.0)
        });
    }
    // Cut at the column: a long translated label ran into the next one.
    button(container(content).clip(true))
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

/// An icon-only toolbar button names itself in a tooltip; a labelled one
/// needs none (#2610).
fn with_tip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: Cow<'static, str>,
    compact: bool,
) -> Element<'a, Message> {
    if compact {
        tooltip(content, name_tip_owned(label), tooltip::Position::Bottom).into()
    } else {
        content.into()
    }
}

fn toolbar_btn<'a>(
    icon: &'static [u8],
    label: Cow<'static, str>,
    msg: Message,
    compact: bool,
) -> Element<'a, Message> {
    let mut content = row![crate::ui::icons::themed(icon, 12.0)]
        .spacing(5)
        .align_y(iced::Center);
    if !compact {
        content = content.push(text(label.clone()).size(11));
    }
    let b = button(content)
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
        .padding(if compact { [4, 6] } else { [4, 10] });
    with_tip(b, label, compact)
}

/// A thin upright line between toolbar groups.
fn toolbar_divider<'a>() -> Element<'a, Message> {
    container(
        container(iced::widget::Space::new().width(1).height(18)).style(|theme: &Theme| {
            container::Style {
                background: Some(Background::Color(theme.palette().background.neutral.color)),
                ..Default::default()
            }
        }),
    )
    .padding([0, 3])
    .into()
}

/// A toolbar button with only a label.
fn toolbar_text_btn<'a>(label: Cow<'static, str>, msg: Message) -> Element<'a, Message> {
    button(text(label).size(11))
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
    compact: bool,
) -> Element<'a, Message> {
    let mut content = row![if enabled {
        crate::ui::icons::themed(icon, 12.0)
    } else {
        crate::ui::icons::themed_disabled(icon, 12.0)
    }]
    .spacing(5)
    .align_y(iced::Center);
    if !compact {
        content = content.push(if enabled {
            text(label.clone()).size(11)
        } else {
            text(label.clone())
                .size(11)
                .style(|theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.palette().background.base.text.scale_alpha(0.42)),
                })
        });
    }
    let mut b = button(content)
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
        .padding(if compact { [4, 6] } else { [4, 10] });
    if enabled {
        b = b.on_press(msg);
    }
    with_tip(b, label, compact)
}

// ── Layer row ─────────────────────────────────────────────────────────────

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

/// A tooltip with owned text, for the toolbar's icon-only buttons.
fn name_tip_owned<'a>(label: Cow<'static, str>) -> Element<'a, Message> {
    container(text(label).size(FONT_SZ))
        .padding([3, 7])
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

/// The description field being typed in; leaving it applies the text.
pub fn description_input_id() -> iced::widget::Id {
    iced::widget::Id::new("layer-description-input")
}

/// The layer-name field while a rename is open; a click that moves focus
/// elsewhere commits the typed name.
pub fn rename_input_id() -> iced::widget::Id {
    iced::widget::Id::new("layer-rename-input")
}

/// The transparency field being typed in; leaving it applies the value.
pub fn transparency_input_id() -> iced::widget::Id {
    iced::widget::Id::new("layer-transparency-input")
}

#[allow(clippy::too_many_arguments)]
fn layer_row<'a>(
    index: usize,
    layer: &'a Layer,
    is_selected: bool,
    is_current: bool,
    is_editing: bool,
    edit_buf: &'a str,
    color_picker_open: bool,
    trans_buf: Option<&'a str>,
    desc_buf: Option<&'a str>,
    lt_combo: Option<&'a combo_box::State<LinetypeItem>>,
    lw_combo_state: Option<&'a combo_box::State<LwItem>>,
    vp_cols: &'a [VpCol],
    active_vp: Option<Handle>,
    name_col_w: f32,
    columns: &[LayerSortCol],
    eye_on: bool,
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

    // Current: a tick; with objects: a dot; empty: a ring (#2610).
    let status_dot: Element<'_, Message> = if is_current {
        crate::ui::icons::themed(crate::ui::icons::CHECK, 13.0)
    } else {
        let filled = layer.objects > 0;
        container(iced::widget::Space::new().width(7).height(7))
            .style(move |theme: &Theme| {
                let ink = theme.palette().background.base.text.scale_alpha(0.7);
                container::Style {
                    background: filled.then_some(Background::Color(ink)),
                    border: Border {
                        color: ink,
                        width: 1.5,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            })
            .into()
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
        // A click selects the row like the rest of it (Ctrl/Shift extend);
        // a double click or F2 renames (#2610).
        let name_btn = mouse_area(
            container(
                text(crate::ui::text_util::elide(&layer.name, name_budget))
                    .size(FONT_SZ)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .clip(true)
            .padding(Padding {
                top: COMBO_PAD_V,
                bottom: COMBO_PAD_V,
                left: 4.0,
                right: 4.0,
            })
            .height(Length::Fixed(ROW_H))
            .width(Length::Fixed(name_col_w)),
        )
        .on_press(Message::LayerSelect(lname.clone()))
        .on_double_click(Message::LayerRenameStart(lname.clone()));
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
                flat: true,
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
            .input_style(cell_field_style),
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
        .input_style(cell_field_style)
        .into()
    } else {
        text(cur_lw_item.to_string())
            .size(FONT_SZ)
            .style(muted_style)
            .width(Length::Fixed(COL_LW))
            .into()
    };

    // Transparency cell: typing only fills the field; Enter or leaving it
    // applies the value as one undo step.
    let trans_str = trans_buf.map_or_else(|| layer.transparency.to_string(), str::to_owned);
    let lname_trans = lname.clone();
    let mut trans_cell = text_input("0", &trans_str)
        .on_input(move |s| Message::LayerTransparencyEdit(lname_trans.clone(), s))
        .on_submit(Message::LayerTransparencyCommit)
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 4.0,
            right: 4.0,
        })
        .style(cell_field_style)
        .width(Length::Fixed(COL_TRANS));
    if trans_buf.is_some() {
        trans_cell = trans_cell.id(transparency_input_id());
    }

    // Description: typed like the transparency, applied on Enter or leaving.
    let desc_str = desc_buf.unwrap_or(layer.description.as_str());
    let lname_desc = lname.clone();
    let mut desc_cell = text_input("—", desc_str)
        .on_input(move |s| Message::LayerDescriptionEdit(lname_desc.clone(), s))
        .on_submit(Message::LayerDescriptionCommit)
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 4.0,
            right: 4.0,
        })
        .style(cell_field_style)
        .width(Length::Fixed(COL_DESC));
    if desc_buf.is_some() {
        desc_cell = desc_cell.id(description_input_id());
    }

    // Objects: the count, and how many sit inside blocks when some do.
    let objects_text = text(layer.objects.to_string())
        .size(FONT_SZ)
        .width(Length::Fixed(COL_OBJECTS))
        .align_x(iced::alignment::Horizontal::Right);
    let objects_cell: Element<'_, Message> = if layer.in_blocks > 0 {
        tooltip(
            objects_text,
            name_tip_owned(crate::tf!("{} of them in blocks", layer.in_blocks)),
            tooltip::Position::FollowCursor,
        )
        .into()
    } else {
        objects_text.into()
    };

    let mut row_content = row![
        // A double click on the status makes the layer current.
        mouse_area(
            container(status_dot)
                .width(COL_STATUS)
                .height(Length::Fixed(ROW_H))
                .align_x(iced::Center)
                .align_y(iced::Center),
        )
        .on_double_click(Message::LayerSetCurrent),
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
        (LayerSortCol::Description, desc_cell.into()),
        (LayerSortCol::Objects, objects_cell),
    ];
    for col in columns {
        if let Some(at) = cells.iter().position(|(c, _)| c == col) {
            let (_, cell) = cells.swap_remove(at);
            row_content = row_content.push(cell);
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

    // The eye: this layer's objects lit in the drawing, the selection kept.
    let eye = button(if eye_on {
        crate::ui::icons::themed_primary(crate::ui::icons::EYE, 13.0)
    } else {
        crate::ui::icons::themed_disabled(crate::ui::icons::EYE, 13.0)
    })
    .on_press(Message::LayerEye(lname.clone()))
    .padding([3, 4])
    .style(move |theme: &Theme, status| {
        let palette = theme.palette();
        button::Style {
            background: matches!(status, button::Status::Hovered)
                .then(|| Background::Color(palette.background.strong.color)),
            border: Border {
                color: if eye_on {
                    palette.primary.base.color
                } else {
                    Color::TRANSPARENT
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        }
    });
    row_content = row_content.push(iced::widget::Space::new().width(Fill));
    row_content = row_content.push(container(eye).width(COL_EYE).align_x(iced::Center));

    mouse_area(
        container(row_content)
            .style(move |theme: &Theme| {
                let palette = theme.palette();
                let (color, text_color) = if is_selected {
                    selection_colors(theme)
                } else if index % 2 == 0 {
                    (palette.background.base.color, palette.background.base.text)
                } else {
                    (palette.background.weak.color, palette.background.weak.text)
                };
                container::Style {
                    background: Some(Background::Color(color)),
                    text_color: Some(text_color),
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
    .on_release(Message::LayerDragEnd)
    .into()
}

/// A table cell's field: plain text until hovered or typed in (#2610
/// mockup), then a field.
fn cell_field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = field_style_with_radius(theme, status, 2.0);
    style.background = Background::Color(Color::TRANSPARENT);
    if matches!(status, text_input::Status::Active | text_input::Status::Disabled) {
        style.border.color = Color::TRANSPARENT;
    }
    style
}

/// The selection colour of the rows and the tree: the accent mixed into
/// the background, quiet in a dark theme as in the mockup, with the
/// background's text colour.
pub fn selection_colors(theme: &Theme) -> (Color, Color) {
    let palette = theme.palette();
    let bg = palette.background.base.color;
    let accent = palette.primary.base.color;
    let dark = bg.r * 0.299 + bg.g * 0.587 + bg.b * 0.114 < 0.5;
    let t = if dark { 0.32 } else { 0.18 };
    let mix = |a: f32, b: f32| a + (b - a) * t;
    (
        Color::from_rgb(mix(bg.r, accent.r), mix(bg.g, accent.g), mix(bg.b, accent.b)),
        palette.background.base.text,
    )
}

/// The tree's rename field, focused when a rename starts.
pub fn tree_rename_input_id() -> iced::widget::Id {
    iced::widget::Id::new("layer-tree-rename")
}

/// A tree line: flat, the chosen one in the selection colour, a light
/// hover.
fn tree_line_style(theme: &Theme, status: button::Status, chosen: bool) -> button::Style {
    let palette = theme.palette();
    let pair = if chosen {
        Some(selection_colors(theme))
    } else if matches!(status, button::Status::Hovered) {
        Some((palette.background.weak.color, palette.background.weak.text))
    } else {
        None
    };
    button::Style {
        background: pair.map(|p| Background::Color(p.0)),
        text_color: pair.map_or(palette.background.base.text, |p| p.1),
        border: Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A line of a right-click menu, with its key on the right when it has one.
/// It acts on the press: the menu's content is built anew whenever the view
/// is, which drops a button's pressed state before the release it would act
/// on, so a button there did nothing.
fn menu_item(
    label: Cow<'static, str>,
    msg: Message,
    indent: f32,
    key: Option<&'static str>,
    ticked: bool,
) -> Element<'static, Message> {
    let line = move |hot: bool| {
        let mut content = row![].spacing(6).align_y(iced::Center);
        // The tick column, as in the mockup: groups the row is in.
        if indent > 0.0 {
            content = content.push(
                container(if ticked {
                    crate::ui::icons::themed(crate::ui::icons::CHECK, 11.0)
                } else {
                    iced::widget::Space::new().width(11).into()
                })
                .width(12),
            );
        }
        content = content.push(text(label.clone()).size(FONT_SZ).width(Fill));
        if let Some(key) = key {
            content = content.push(text(key).size(FONT_SZ).style(muted_style));
        }
        container(content)
            .width(Fill)
            .padding(Padding {
                top: 4.0,
                bottom: 4.0,
                left: 12.0 + indent,
                right: 12.0,
            })
            .style(move |theme: &Theme| {
                let palette = theme.palette();
                container::Style {
                    background: hot.then(|| Background::Color(palette.background.strong.color)),
                    text_color: Some(palette.background.base.text),
                    ..Default::default()
                }
            })
    };
    mouse_area(iced::widget::hover(line(false), line(true)))
        .on_press(msg)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn menu_separator() -> Element<'static, Message> {
    container(iced::widget::Space::new())
        .height(1)
        .width(Fill)
        .padding([4, 0])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.neutral.color)),
            ..Default::default()
        })
        .into()
}

fn menu_frame(items: Vec<Element<'static, Message>>, width: f32) -> Element<'static, Message> {
    container(
        iced::widget::Column::with_children(items)
            .spacing(0)
            .width(Length::Fixed(width)),
    )
    .padding([4, 0])
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// A filter tree node's right-click menu: one level (#2610).
fn tree_menu(node: &TreeNode) -> Element<'static, Message> {
    let entry = |label: Cow<'static, str>, action: TreeAction| -> Element<'static, Message> {
        menu_item(label, Message::LayerTreeAction(node.clone(), action), 0.0, None, false)
    };
    let mut items: Vec<Element<'static, Message>> = Vec::new();
    match node {
        TreeNode::Filter(_) => {
            items.push(entry(t!("Edit…"), TreeAction::Edit));
            items.push(menu_item(
                t!("Rename"),
                Message::LayerTreeAction(node.clone(), TreeAction::Rename),
                0.0,
                Some("F2"),
                false,
            ));
            items.push(entry(t!("Delete"), TreeAction::Delete));
            items.push(menu_separator());
        }
        TreeNode::Group(_) => {
            items.push(menu_item(
                t!("Rename"),
                Message::LayerTreeAction(node.clone(), TreeAction::Rename),
                0.0,
                Some("F2"),
                false,
            ));
            items.push(entry(
                crate::tr!("layer-groups", "delete").into(),
                TreeAction::Delete,
            ));
            items.push(menu_separator());
        }
        TreeNode::Unused => {
            items.push(entry(t!("Delete with Purge…"), TreeAction::Purge));
            items.push(menu_separator());
        }
        _ => {}
    }
    items.extend([
        entry(t!("Turn all on"), TreeAction::On),
        entry(t!("Turn all off"), TreeAction::Off),
        entry(t!("Freeze all"), TreeAction::Freeze),
        entry(t!("Thaw all"), TreeAction::Thaw),
        entry(t!("Lock all"), TreeAction::Lock),
        entry(t!("Unlock all"), TreeAction::Unlock),
    ]);
    menu_frame(items, 200.0)
}

/// A layer row's right-click menu, as in the mockup. One level: the groups
/// open under "Add to group", inside the menu, ticked where the layer is in
/// one; "New group" closes that list.
fn row_menu(name: &str, groups: &[(String, bool)], in_group: bool) -> Element<'static, Message> {
    let entry = |label: Cow<'static, str>, action: LayerMenuAction, key: Option<&'static str>| {
        menu_item(label, Message::LayerMenu(name.to_string(), action), 0.0, key, false)
    };
    let mut items: Vec<Element<'static, Message>> = vec![
        entry(t!("Set Current"), LayerMenuAction::SetCurrent, None),
        entry(t!("New"), LayerMenuAction::New, None),
        entry(t!("Rename"), LayerMenuAction::Rename, Some("F2")),
        entry(t!("Delete"), LayerMenuAction::Delete, Some("Del")),
        menu_separator(),
        entry(t!("Select Objects"), LayerMenuAction::SelectObjects, None),
        entry(t!("Filter objects..."), LayerMenuAction::FilterObjects, None),
        entry(t!("Merge selected layers here"), LayerMenuAction::MergeHere, None),
        menu_separator(),
        container(
            row![
                text(crate::tr!("layer-groups", "add-to"))
                    .size(FONT_SZ)
                    .width(Fill),
                crate::ui::icons::themed_arrow_down(8.0),
            ]
            .align_y(iced::Center),
        )
        .padding([4, 12])
        .into(),
    ];
    for (group, ticked) in groups {
        items.push(menu_item(
            Cow::Owned(group.clone()),
            Message::LayerMenu(name.to_string(), LayerMenuAction::AddToGroup(group.clone())),
            14.0,
            None,
            *ticked,
        ));
    }
    items.push(menu_item(
        crate::tr!("layer-groups", "new-group").into(),
        Message::LayerMenu(name.to_string(), LayerMenuAction::NewGroup),
        14.0,
        None,
        false,
    ));
    if in_group {
        items.push(entry(
            crate::tr!("layer-groups", "remove").into(),
            LayerMenuAction::RemoveFromGroup,
            None,
        ));
    }
    menu_frame(items, 230.0)
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
const COL_ICON: f32 = 36.0;
const COL_COLOR: f32 = 90.0;
const COL_LT: f32 = 100.0;
const COL_LW: f32 = 80.0;
const COL_TRANS: f32 = 56.0;
const COL_DESC: f32 = 200.0;
/// The filter tree beside the table.
const TREE_W: f32 = 210.0;
const COL_OBJECTS: f32 = 60.0;
/// The status column (current, used, empty).
const COL_STATUS: f32 = 36.0;
/// The eye button at the row's end.
const COL_EYE: f32 = 30.0;
/// The compact toolbar's "…" menu.
const MORE_MENU_W: f32 = 190.0;

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

#[cfg(test)]
mod width_band_tests {
    use super::*;

    /// #2610: labelled buttons only when wide; the narrow pallet keeps On,
    /// Freeze, Lock and Color.
    #[test]
    fn width_bands_and_narrow_columns() {
        assert_eq!(WidthBand::of(900.0), WidthBand::Wide);
        assert_eq!(WidthBand::of(600.0), WidthBand::Medium);
        assert_eq!(WidthBand::of(300.0), WidthBand::Narrow);
        let narrow = shown_columns(WidthBand::Narrow);
        assert!(narrow.iter().all(|col| NARROW_COLUMNS.contains(col)));
        assert!(narrow.contains(&LayerSortCol::On) && narrow.contains(&LayerSortCol::Color));
        let wide = shown_columns(WidthBand::Wide);
        assert!(wide.contains(&LayerSortCol::Description) && wide.contains(&LayerSortCol::Objects));
    }
}

#[cfg(test)]
mod more_menu_tests {
    use super::*;
    use iced_test::Simulator;

    /// #2610: the narrow pallet's "…" menu opens below the button at its own
    /// width; it had taken the button's ~30 px and showed no items.
    #[test]
    fn more_menu_opens_below_at_full_width() {
        let panel = LayerPanel {
            more_menu: true,
            ..LayerPanel::default()
        };
        let mut ui = Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(360.0, 600.0),
            panel.view_content(140.0, 360.0, None),
        );
        for label in ["New Filter", "Layer States", "Import...", "Advanced…"] {
            let bounds = ui
                .find(label.to_string())
                .unwrap_or_else(|_| panic!("{label:?} is not in the menu"))
                .visible_bounds()
                .unwrap_or_else(|| panic!("{label:?} is not visible"));
            assert!(bounds.width > 40.0, "{label:?} is squeezed: {bounds:?}");
            assert!(
                bounds.y > 24.0,
                "{label:?} is not below the toolbar: {bounds:?}"
            );
        }
    }
}
