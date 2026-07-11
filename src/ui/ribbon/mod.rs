// Ribbon — tab bar + 3-row tool area.
//
// Button sizes:
//   LargeTool / LargeDropdown  — full ribbon height (3 rows), icon + label [+ ▾]
//   Tool / Dropdown            — 1-row height, icon only [+ ▾ on right]
//
// Dropdown items within a group are collected into columns of 3 rows.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use rustc_hash::FxHashMap as HashMap;

use acadrust::types::{Color as AcadColor, LineWeight};
use iced::widget::{button, column, container, mouse_area, row, scrollable, svg, text};
use iced::{Background, Border, Color, Element, Fill, Length, Padding, Theme};

use crate::app::Message;
use crate::modules::{CadModule, IconKind, RibbonGroup, RibbonItem};
use crate::plugin::all_ribbon_modules;
use crate::ui::properties::{lw_options, LinetypeItem};

mod widgets;
use widgets::{StyleContext, *};
mod collapse;
use collapse::{CollapsePanels, Panel};
pub use collapse::CollapseMode;
use crate::ui::wrap_bar::{PosReport, WrapBar, WrapFlow};

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
    /// File tabs (FILETAB) visibility — drives the File Tabs button highlight.
    pub show_file_tabs: bool,
    /// Layout tabs (LAYOUTTAB) visibility — drives the Layout Tabs button highlight.
    pub show_layout_tabs: bool,
    pub open_dropdown: Option<String>,
    /// Title of the collapsed panel whose flyout is currently open, if any.
    pub collapsed_open: Option<String>,
    /// Last tool used from each panel (panel title → tool id). A collapsed panel
    /// shows this tool on its button, defaulting to the panel's first tool.
    last_panel_tool: HashMap<&'static str, &'static str>,
    last_cmd: HashMap<&'static str, &'static str>,
    pub layer_names: Vec<String>,
    pub active_layer: String,
    pub layer_infos: Vec<LayerInfo>,
    /// Active object color — ACI / ByLayer / ByBlock.
    pub active_color: AcadColor,
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
    /// User-chosen panel density (persisted). `Auto` sizes by window width.
    collapse_mode: CollapseMode,
    /// Set by `CollapsePanels` when the tool row is in its tight state; the mode
    /// selector hides itself then to give the cramped tab row its space back.
    collapse_tight: Arc<AtomicBool>,
}

/// Per-layer display data shown in the ribbon layer dropdown.
#[derive(Clone, Debug)]
pub struct LayerInfo {
    pub name: String,
    pub color: Color,
    pub visible: bool,
    pub frozen: bool,
    pub locked: bool,
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
            show_file_tabs: true,
            show_layout_tabs: true,
            open_dropdown: None,
            collapsed_open: None,
            last_panel_tool: HashMap::default(),
            last_cmd: HashMap::default(),
            // Empty until a document is open — populated by sync_ribbon_layers.
            layer_names: vec![],
            active_layer: String::new(),
            layer_infos: vec![],
            active_color: AcadColor::ByLayer,
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
            tab_bar_h: Arc::new(AtomicU32::new(28.0f32.to_bits())),
            tool_bar_h: Arc::new(AtomicU32::new(TOOL_BAR_H.to_bits())),
            collapse_mode: CollapseMode::default(),
            collapse_tight: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Change the tool-panel density. Persistence is handled by the caller via
    /// the consolidated app config (`save_config`).
    pub fn set_collapse_mode(&mut self, mode: CollapseMode) {
        self.collapse_mode = mode;
    }

    /// The current tool-panel density (for saving into the app config).
    pub fn collapse_mode(&self) -> CollapseMode {
        self.collapse_mode
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
        self.layer_names = infos.iter().map(|l| l.name.clone()).collect();
        self.layer_infos = infos;
    }

    pub fn set_available_linetypes(&mut self, items: Vec<LinetypeItem>) {
        self.available_linetypes = items;
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
    }
    pub fn activate_tool(&mut self, id: &str) {
        self.active_tool = Some(id.to_string());
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
    pub fn set_file_tabs(&mut self, on: bool) {
        self.show_file_tabs = on;
    }
    pub fn set_layout_tabs(&mut self, on: bool) {
        self.show_layout_tabs = on;
    }
    /// Snapshot of every ribbon toggle's live state, for the render path.
    fn toggle_state(&self) -> widgets::ToggleState {
        use widgets::ToggleState;
        ToggleState {
            wireframe: self.wireframe,
            ortho_mode: self.ortho_mode,
            show_viewcube: self.show_viewcube,
            show_ucs_icon: self.show_ucs_icon,
            show_properties: self.show_properties,
            show_file_tabs: self.show_file_tabs,
            show_layout_tabs: self.show_layout_tabs,
        }
    }

    pub fn toggle_dropdown(&mut self, id: &str) {
        if self.open_dropdown.as_deref() == Some(id) {
            self.open_dropdown = None;
        } else {
            self.open_dropdown = Some(id.to_string());
        }
    }
    pub fn close_dropdown(&mut self) {
        self.open_dropdown = None;
        self.collapsed_open = None;
    }

    /// Toggle the flyout of a collapsed ribbon panel (identified by its title).
    pub fn toggle_collapsed_panel(&mut self, id: &str) {
        if self.collapsed_open.as_deref() == Some(id) {
            self.collapsed_open = None;
        } else {
            self.collapsed_open = Some(id.to_string());
        }
    }

    /// Record `tool_id` as the last-used tool of whichever active-module panel
    /// contains it, so a collapsed panel shows that tool on its button.
    pub fn note_panel_tool(&mut self, tool_id: &str) {
        if let Some(module) = self.modules.get(self.active) {
            for group in module.ribbon_groups() {
                if let Some(id) = group
                    .tools
                    .iter()
                    .filter_map(item_id)
                    .find(|id| *id == tool_id)
                {
                    self.last_panel_tool.insert(group.title, id);
                    return;
                }
            }
        }
    }

    /// Returns the index of the Layout module in the modules list.
    #[allow(dead_code)] // layout-tab helpers; not yet wired
    pub fn layout_module_index(&self) -> Option<usize> {
        self.modules.iter().position(|m| m.id() == "layout")
    }

    /// Returns true if the currently active tab is the Layout module.
    #[allow(dead_code)]
    pub fn active_is_layout(&self) -> bool {
        self.modules
            .get(self.active)
            .map(|m| m.id() == "layout")
            .unwrap_or(false)
    }

    pub fn select_dropdown_item(&mut self, dropdown_id: &'static str, cmd: &'static str) {
        self.last_cmd.insert(dropdown_id, cmd);
        self.open_dropdown = None;
    }

    // ── View ──────────────────────────────────────────────────────────────

    pub fn view(
        &self,
        is_paper: bool,
        undo_count: usize,
        redo_count: usize,
    ) -> Element<'_, Message> {
        // ── Quick-access file commands + undo/redo, one merged flow ────────
        let lead = WrapFlow::new(vec![
            quick_access_btn(crate::ui::icons::DOC_NEW, "New", "NEW").into(),
            quick_access_btn(crate::ui::icons::FOLDER_OPEN, "Open", "OPEN").into(),
            quick_access_btn(crate::ui::icons::SAVE, "Save", "SAVE").into(),
            quick_access_btn(crate::ui::icons::FILE_EXPORT, "Save As", "SAVEAS").into(),
            quick_access_btn(crate::ui::icons::PRINT, "Print", "PRINT").into(),
            render_history_control("Undo", UNDO_HISTORY_ID, undo_count, &self.open_dropdown).into(),
            render_history_control("Redo", REDO_HISTORY_ID, redo_count, &self.open_dropdown).into(),
        ])
        .spacing_x(TOP_HIST_GAP)
        .row_h(28.0);

        // The quick-access flow and the tabs flow each flex-wrap; WrapBar stacks
        // them so a wrapped tab never shares a row with a quick-access button.

        let tab_items = self.modules.iter().enumerate().fold(
            Vec::<Element<'_, Message>>::new(),
            |mut acc, (i, module)| {
                // The Layout module no longer has a ribbon tab — its paper-space
                // tools live in the right-edge side toolbar (see ui::side_toolbar).
                if module.id() == "layout" {
                    return acc;
                }

                let is_active = i == self.active;
                let is_contextual = module.id() == "layout";
                let accent = if is_contextual {
                    ACCENT_GOLD
                } else {
                    ACCENT_BLUE
                };
                let text_inactive = if is_contextual {
                    Color {
                        r: 0.90,
                        g: 0.72,
                        b: 0.30,
                        a: 1.0,
                    }
                } else {
                    Color {
                        r: 0.75,
                        g: 0.75,
                        b: 0.75,
                        a: 1.0,
                    }
                };
                let hover_bg = if is_contextual {
                    Color {
                        r: 0.28,
                        g: 0.24,
                        b: 0.12,
                        a: 1.0,
                    }
                } else {
                    Color {
                        r: 0.25,
                        g: 0.25,
                        b: 0.25,
                        a: 1.0,
                    }
                };
                let btn = container(
                    button(text(module.title()).size(12))
                        .on_press(Message::RibbonSelectTab(i))
                        .style(move |_: &Theme, status| button::Style {
                            background: Some(Background::Color(match (is_active, status) {
                                (true, _) => RIBBON_BG,
                                (false, button::Status::Hovered) => hover_bg,
                                _ => Color::TRANSPARENT,
                            })),
                            text_color: if is_active {
                                Color::WHITE
                            } else {
                                text_inactive
                            },
                            border: Border {
                                color: if is_active {
                                    accent
                                } else {
                                    Color::TRANSPARENT
                                },
                                width: if is_active { 2.0 } else { 0.0 },
                                radius: 0.0.into(),
                            },
                            shadow: iced::Shadow::default(),
                            snap: false,
                        })
                        .padding([5, 14]),
                )
                .style(move |_: &Theme| container::Style {
                    border: Border {
                        color: if is_active {
                            accent
                        } else {
                            Color::TRANSPARENT
                        },
                        width: if is_active { 2.0 } else { 0.0 },
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                });
                acc.push(btn.into());
                acc
            },
        );

        // Tabs may squeeze their gaps to fit before wrapping: from the normal 6px
        // down to -12px on a narrow (e.g. phone) tab row, tucking neighbours into
        // each other's 14px side padding without overlapping the labels. When the
        // row has room the gap stays 6px, so wide/desktop layouts are unchanged.
        let tabs = WrapFlow::new(tab_items)
            .spacing_x(6.0)
            .min_spacing_x(-12.0)
            .row_h(28.0);

        // Panel-density selector, pinned to the right edge of the tab row: a bare
        // ▾ button that opens a list of modes (see `dropdown_overlay`). `Auto`
        // sizes panels to the window; the others force one density. The choice is
        // persisted (see `Ribbon::set_collapse_mode`). It hides itself once the
        // tool row is tight, giving the cramped tab row its space back.
        let dd_open = self.open_dropdown.as_deref() == Some(COLLAPSE_MODE_ID);
        let mode_btn = button(crate::ui::icons::arrow_down(10.0, ARROW_COLOR))
            .on_press(Message::ToggleRibbonDropdown(COLLAPSE_MODE_ID.to_string()))
            .style(move |_: &Theme, status| top_hist_btn_style(true, dd_open, status))
            .height(24)
            .padding([2, 8]);
        let mode_dd = PosReport::new(COLLAPSE_MODE_ID, mode_btn);

        let mut tab_row = row![container(
            WrapBar::new(lead.into(), tabs.into())
                .spacing(6.0)
                .report_height(self.tab_bar_h.clone()),
        )
        .width(Length::Fill)]
        .spacing(6.0)
        .align_y(iced::Center);
        if !self.collapse_tight.load(Ordering::Relaxed) {
            tab_row = tab_row.push(mode_dd);
        }

        let tab_bar = container(tab_row)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(TOPBAR_BG)),
                ..Default::default()
            })
            .padding(Padding {
                right: 6.0,
                ..Padding::ZERO
            })
            .width(Length::Fill);

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
        let tool_area: Element<'_, Message> =
            if let Some(module) = self.modules.get(effective_active) {
                let groups = module.ribbon_groups();
                let style_ctx = StyleContext {
                    text_style_names: self.text_style_names.clone(),
                    active_text_style: self.active_text_style.clone(),
                    dim_style_names: self.dim_style_names.clone(),
                    active_dim_style: self.active_dim_style.clone(),
                    mleader_style_names: self.mleader_style_names.clone(),
                    active_mleader_style: self.active_mleader_style.clone(),
                    table_style_names: self.table_style_names.clone(),
                    active_table_style: self.active_table_style.clone(),
                };

                // Adaptive tool area: panels sit on one row; when they don't all
                // fit they degrade from the right — a panel's large buttons first
                // shrink to compact icon columns, then it collapses to a ▾ flyout
                // button. See `CollapsePanels`.
                let panels: Vec<Panel<'_>> = groups
                    .iter()
                    .map(|g| {
                        let ts = self.toggle_state();
                        Panel {
                        id: g.title.to_string(),
                        full: render_group(
                            false,
                            g,
                            &self.active_tool,
                            &self.open_dropdown,
                            &self.last_cmd,
                            ts,
                            &self.layer_infos,
                            &self.active_layer,
                            self.active_color,
                            &self.active_linetype,
                            self.active_lineweight,
                            &style_ctx,
                        ),
                        compact: render_group(
                            true,
                            g,
                            &self.active_tool,
                            &self.open_dropdown,
                            &self.last_cmd,
                            ts,
                            &self.layer_infos,
                            &self.active_layer,
                            self.active_color,
                            &self.active_linetype,
                            self.active_lineweight,
                            &style_ctx,
                        ),
                        button: collapse_button(
                            g,
                            self.last_panel_tool.get(g.title).copied(),
                            &self.active_tool,
                            &self.open_dropdown,
                            &self.last_cmd,
                            ts,
                            &self.layer_infos,
                            &self.active_layer,
                            self.active_color,
                            &self.active_linetype,
                            self.active_lineweight,
                            &style_ctx,
                            false,
                        ),
                        tight: collapse_button(
                            g,
                            self.last_panel_tool.get(g.title).copied(),
                            &self.active_tool,
                            &self.open_dropdown,
                            &self.last_cmd,
                            ts,
                            &self.layer_infos,
                            &self.active_layer,
                            self.active_color,
                            &self.active_linetype,
                            self.active_lineweight,
                            &style_ctx,
                            true,
                        ),
                        flyout: container(render_group(
                            false,
                            g,
                            &self.active_tool,
                            &self.open_dropdown,
                            &self.last_cmd,
                            ts,
                            &self.layer_infos,
                            &self.active_layer,
                            self.active_color,
                            &self.active_linetype,
                            self.active_lineweight,
                            &style_ctx,
                        ))
                        .style(|_: &Theme| container::Style {
                            background: Some(Background::Color(RIBBON_BG)),
                            border: Border {
                                color: BORDER_DARK,
                                width: 1.0,
                                radius: 0.0.into(),
                            },
                            ..Default::default()
                        })
                        .into(),
                    }
                    })
                    .collect();
                CollapsePanels::new(panels, self.collapsed_open.clone(), TOOL_BAR_H, BORDER_DARK)
                    .report_height(self.tool_bar_h.clone())
                    .report_tight(self.collapse_tight.clone())
                    .mode(self.collapse_mode)
                    .into()
            } else {
                text("").into()
            };

        let tool_bar = container(tool_area)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(RIBBON_BG)),
                border: Border {
                    color: BORDER_DARK,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fill);

        column![tab_bar, tool_bar].into()
    }

    // ── Dropdown overlay ──────────────────────────────────────────────────

    pub fn dropdown_overlay(
        &self,
        undo_labels: &[String],
        redo_labels: &[String],
        win: (f32, f32),
    ) -> Option<Element<'_, Message>> {
        let open_id = self.open_dropdown.as_deref()?;

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
                    button(text(label.clone()).size(11).color(LABEL_ON))
                        .on_press(if is_undo {
                            Message::UndoMany(step)
                        } else {
                            Message::RedoMany(step)
                        })
                        .style(|_: &Theme, status| button::Style {
                            background: Some(Background::Color(match status {
                                button::Status::Hovered | button::Status::Pressed => ROW_HOVER,
                                _ => Color::TRANSPARENT,
                            })),
                            ..Default::default()
                        })
                        .width(Fill)
                        .padding([5, 10])
                        .into()
                })
                .collect();

            let panel = container(column(rows))
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(PANEL_BG)),
                    border: Border {
                        color: PANEL_BORDER,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
                .width(Length::Fixed(170.0));

            let (align_right, h_pad, top) = self.dd_anchor(open_id, 170.0, win.0);
            let positioned = position_ribbon_dropdown(panel.into(), align_right, h_pad, top);

            return Some(dropdown_backdrop(positioned));
        }

        if open_id == COLLAPSE_MODE_ID {
            const W: f32 = 150.0;
            let current = self.collapse_mode;
            let rows: Vec<Element<Message>> = CollapseMode::ALL
                .iter()
                .map(|&m| {
                    let mark: Element<Message> = if m == current {
                        crate::ui::icons::tinted(crate::ui::icons::CHECK, 11.0, CHECK_COLOR)
                    } else {
                        iced::widget::Space::new().width(0).into()
                    };
                    button(
                        row![
                            container(mark).width(Length::Fixed(16.0)),
                            text(m.label()).size(11).color(LABEL_ON),
                        ]
                        .spacing(4)
                        .align_y(iced::Center),
                    )
                    .on_press(Message::SetRibbonCollapseMode(m))
                    .style(|_: &Theme, status| button::Style {
                        background: Some(Background::Color(match status {
                            button::Status::Hovered | button::Status::Pressed => ROW_HOVER,
                            _ => Color::TRANSPARENT,
                        })),
                        ..Default::default()
                    })
                    .width(Fill)
                    .padding([5, 10])
                    .into()
                })
                .collect();

            let panel = container(column(rows))
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(PANEL_BG)),
                    border: Border {
                        color: PANEL_BORDER,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
                .width(Length::Fixed(W));

            let (align_right, h_pad, top) = self.dd_anchor(open_id, W, win.0);
            let positioned = position_ribbon_dropdown(panel.into(), align_right, h_pad, top);
            return Some(dropdown_backdrop(positioned));
        }

        if open_id == LAYER_COMBO_ID {
            return self.layer_combo_overlay(win);
        }

        if open_id == PROP_COLOR_ID {
            return self.prop_color_overlay(win);
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

        let module = self.modules.get(self.active)?;
        let groups = module.ribbon_groups();
        let mut items_list: Option<Vec<(&'static str, &'static str, IconKind)>> = None;
        let mut dd_default = "";
        let mut dd_id: &'static str = "";

        'outer: for group in groups {
            for item in &group.tools {
                let (id, items, default) = match item {
                    RibbonItem::Dropdown {
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
                let checkmark: Element<'_, Message> = container(if is_current {
                    crate::ui::icons::tinted(crate::ui::icons::CHECK, 11.0, CHECK_COLOR)
                } else {
                    iced::widget::Space::new().width(0).into()
                })
                .width(Length::Fixed(14.0))
                .into();
                let icon_el: Element<Message> = match *item_icon {
                    IconKind::Glyph(s) => text(s)
                        .size(13)
                        .color(ICON_COLOR)
                        .width(Length::Fixed(20.0))
                        .into(),
                    IconKind::Svg(bytes) => {
                        let handle = svg::Handle::from_memory(bytes);
                        svg(handle).width(20).height(20).into()
                    }
                };
                let label_el =
                    text(*label)
                        .size(11)
                        .color(if is_current { LABEL_ON } else { LABEL_OFF });

                button(
                    row![checkmark, icon_el, label_el]
                        .spacing(4)
                        .align_y(iced::Center),
                )
                .on_press(Message::DropdownSelectItem {
                    dropdown_id: dd_id,
                    cmd: *cmd,
                })
                .style(|_: &Theme, status| button::Style {
                    background: Some(Background::Color(match status {
                        button::Status::Hovered | button::Status::Pressed => ROW_HOVER,
                        _ => Color::TRANSPARENT,
                    })),
                    ..Default::default()
                })
                .width(Fill)
                .padding([4, 10])
                .into()
            })
            .collect();

        let panel = container(column(rows))
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(PANEL_BG)),
                border: Border {
                    color: PANEL_BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fixed(190.0));

        let (align_right, h_pad, top) = self.dd_anchor(open_id, 190.0, win.0);
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
            button(crate::ui::icons::raw(bytes, 14.0))
                .on_press(msg)
                .style(|_: &Theme, status| button::Style {
                    background: Some(Background::Color(match status {
                        button::Status::Hovered | button::Status::Pressed => ROW_HOVER,
                        _ => Color::TRANSPARENT,
                    })),
                    ..Default::default()
                })
                .padding([2, 4])
                .into()
        };
        let rows: Vec<Element<Message>> = self
            .layer_infos
            .iter()
            .enumerate()
            .map(|(index, info)| {
                let is_active = info.name == self.active_layer;
                let lc = info.color;
                let lv = info.visible;
                let lf = info.frozen;
                let ll = info.locked;
                let name = info.name.clone();

                let swatch = container(text(""))
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(lc)),
                        border: Border {
                            color: SWATCH_BORDER,
                            width: 1.0,
                            radius: 1.0.into(),
                        },
                        ..Default::default()
                    })
                    .width(12)
                    .height(12);

                let vis = icon_btn(
                    crate::ui::icons::layer_visible(lv),
                    Message::LayerToggleVisible(index),
                );
                let freeze = icon_btn(
                    crate::ui::icons::layer_freeze(lf),
                    Message::LayerToggleFreeze(index),
                );
                let lock = icon_btn(
                    crate::ui::icons::layer_lock(ll),
                    Message::LayerToggleLock(index),
                );
                let checkmark: Element<'_, Message> = container(if is_active {
                    crate::ui::icons::tinted(crate::ui::icons::CHECK, 11.0, CHECK_COLOR)
                } else {
                    iced::widget::Space::new().width(0).into()
                })
                .width(Length::Fixed(14.0))
                .into();
                let label =
                    text(&info.name)
                        .size(11)
                        .color(if is_active { LABEL_ON } else { LABEL_OFF });

                // The swatch + label area selects the layer as active; the
                // icon buttons above handle their own toggles.
                let select = button(row![swatch, label].spacing(5).align_y(iced::Center))
                    .on_press(Message::RibbonLayerChanged(name))
                    .style(|_: &Theme, status| button::Style {
                        background: Some(Background::Color(match status {
                            button::Status::Hovered | button::Status::Pressed => ROW_HOVER,
                            _ => Color::TRANSPARENT,
                        })),
                        ..Default::default()
                    })
                    .width(Fill)
                    .padding([4, 4]);

                container(
                    row![checkmark, vis, freeze, lock, select]
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
        let row_count = self.layer_infos.len().max(1);
        let list_h = (row_count as f32 * 26.0).min(420.0);
        let panel = container(scrollable(column(rows)).height(Length::Fixed(list_h)))
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(PANEL_BG)),
                border: Border {
                    color: PANEL_BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fixed(220.0));

        let (align_right, h_pad, top) = self.dd_anchor(LAYER_COMBO_ID, 220.0, win.0);
        let positioned = position_ribbon_dropdown(panel.into(), align_right, h_pad, top);

        Some(dropdown_backdrop(positioned))
    }

    /// Floating popup for an annotate-tab style combo (text / dimension /
    /// multileader / table style). Returns `None` when `open_id` is not a
    /// style combo. Built as an overlay — like the layer combo and the
    /// Draw-tab dropdowns — so the list grows to fit its entries instead of
    /// being clipped to the ribbon-row height. (#153)
    fn style_combo_overlay(&self, open_id: &str, win: (f32, f32)) -> Option<Element<'_, Message>> {
        let groups = self.modules.get(self.active)?.ribbon_groups();

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
            text_style_names: self.text_style_names.clone(),
            active_text_style: self.active_text_style.clone(),
            dim_style_names: self.dim_style_names.clone(),
            active_dim_style: self.active_dim_style.clone(),
            mleader_style_names: self.mleader_style_names.clone(),
            active_mleader_style: self.active_mleader_style.clone(),
            table_style_names: self.table_style_names.clone(),
            active_table_style: self.active_table_style.clone(),
        };
        let active = ctx.active_for(style_key).to_string();

        let row_style = |_: &Theme, status: button::Status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered | button::Status::Pressed => ROW_HOVER,
                _ => Color::TRANSPARENT,
            })),
            ..Default::default()
        };

        let mut rows: Vec<Element<Message>> = ctx
            .names_for(style_key)
            .iter()
            .map(|name| {
                let is_sel = name.as_str() == active.as_str();
                let n = name.clone();
                let checkmark: Element<Message> = container(if is_sel {
                    crate::ui::icons::tinted(crate::ui::icons::CHECK, 11.0, CHECK_COLOR)
                } else {
                    iced::widget::Space::new().width(0).into()
                })
                .width(Length::Fixed(14.0))
                .into();
                button(
                    row![
                        checkmark,
                        text(name.clone()).size(11).color(if is_sel {
                            LABEL_ON
                        } else {
                            LABEL_OFF
                        }),
                    ]
                    .spacing(4)
                    .align_y(iced::Center),
                )
                .on_press(Message::RibbonStyleChanged {
                    key: style_key,
                    name: n,
                })
                .style(row_style)
                .width(Fill)
                .padding([4, 10])
                .into()
            })
            .collect();

        if let Some(mgr) = manager_cmd {
            rows.push(
                button(text("Manage…").size(11).color(LABEL_ON))
                    .on_press(Message::Command(mgr.to_string()))
                    .style(row_style)
                    .width(Fill)
                    .padding([4, 10])
                    .into(),
            );
        }

        let panel = container(column(rows))
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(PANEL_BG)),
                border: Border {
                    color: PANEL_BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fixed(LARGE_W * 2.3));

        let (align_right, h_pad, top) = self.dd_anchor(open_id, LARGE_W * 2.3, win.0);
        Some(dropdown_backdrop(position_ribbon_dropdown(
            panel.into(),
            align_right,
            h_pad,
            top,
        )))
    }

    fn prop_color_overlay(&self, win: (f32, f32)) -> Option<Element<'_, Message>> {
        let picker = crate::ui::color_select::color_list(
            crate::ui::color_select::ColorExtras {
                by_layer: true,
                by_block: true,
            },
            Message::RibbonColorChanged,
            Message::OpenColorWindow(crate::app::ColorPickTarget::Ribbon),
        );

        let panel = container(picker)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(PANEL_BG)),
                border: Border {
                    color: PANEL_BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fixed(200.0));

        let (align_right, h_pad, top) = self.dd_anchor(PROP_COLOR_ID, 200.0, win.0);
        Some(dropdown_backdrop(position_ribbon_dropdown(
            panel.into(),
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
                let check: Element<'_, Message> = container(if is_cur {
                    crate::ui::icons::tinted(crate::ui::icons::CHECK, 11.0, CHECK_COLOR)
                } else {
                    iced::widget::Space::new().width(0).into()
                })
                .width(Length::Fixed(14.0))
                .into();
                let name_col = text(lt.name.clone())
                    .size(11)
                    .color(if is_cur { LABEL_ON } else { LABEL_OFF })
                    .width(Length::Fixed(90.0));
                let art_col = text(lt.art.clone()).size(9).color(Color {
                    r: 0.55,
                    g: 0.55,
                    b: 0.55,
                    a: 1.0,
                });
                let name = lt.name.clone();
                button(
                    row![check, name_col, art_col]
                        .spacing(4)
                        .align_y(iced::Center),
                )
                .on_press(Message::RibbonLinetypeChanged(name))
                .style(|_: &Theme, status| button::Style {
                    background: Some(Background::Color(match status {
                        button::Status::Hovered | button::Status::Pressed => ROW_HOVER,
                        _ => Color::TRANSPARENT,
                    })),
                    ..Default::default()
                })
                .width(Fill)
                .padding([4, 6])
                .into()
            })
            .collect();

        let list = container(scrollable(column(rows)).height(Length::Fixed(200.0)))
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(PANEL_BG)),
                border: Border {
                    color: PANEL_BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
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
                let check: Element<'_, Message> = container(if is_cur {
                    crate::ui::icons::tinted(crate::ui::icons::CHECK, 11.0, CHECK_COLOR)
                } else {
                    iced::widget::Space::new().width(0).into()
                })
                .width(Length::Fixed(14.0))
                .into();
                button(
                    row![
                        check,
                        text(label)
                            .size(11)
                            .color(if is_cur { LABEL_ON } else { LABEL_OFF })
                    ]
                    .spacing(5)
                    .align_y(iced::Center),
                )
                .on_press(Message::RibbonLineweightChanged(item.0))
                .style(|_: &Theme, status| button::Style {
                    background: Some(Background::Color(match status {
                        button::Status::Hovered | button::Status::Pressed => ROW_HOVER,
                        _ => Color::TRANSPARENT,
                    })),
                    ..Default::default()
                })
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
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(PANEL_BG)),
                border: Border {
                    color: PANEL_BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
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

/// Render a single ribbon panel (tools + group label), fixed `TOOL_BAR_H` tall.
/// When `compact`, large tools/dropdowns are drawn as small icon columns.
#[allow(clippy::too_many_arguments)]
fn render_group<'a>(
    compact: bool,
    group: &RibbonGroup,
    active_tool: &Option<String>,
    open_dd: &Option<String>,
    last_cmd: &HashMap<&'static str, &'static str>,
    state: widgets::ToggleState,
    layer_infos: &'a [LayerInfo],
    active_layer: &'a str,
    active_color: AcadColor,
    active_linetype: &'a str,
    active_lineweight: LineWeight,
    style_ctx: &StyleContext,
) -> Element<'a, Message> {
    let mut items_row: Vec<Element<Message>> = Vec::new();
    let mut small_buf: Vec<Element<Message>> = Vec::new();

    for item in &group.tools {
        let is_large = match item {
            RibbonItem::LargeTool(_) | RibbonItem::LargeDropdown { .. } => !compact,
            RibbonItem::LayerComboGroup { .. }
            | RibbonItem::PropertiesGroup { .. }
            | RibbonItem::StyleComboGroup { .. } => true,
            _ => false,
        };

        if is_large {
            flush_small_col(&mut small_buf, &mut items_row);
            items_row.push(render_large(
                item,
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
            ));
        } else {
            small_buf.push(render_small(
                item,
                active_tool,
                open_dd,
                last_cmd,
                state,
            ));
            if small_buf.len() == 3 {
                flush_small_col(&mut small_buf, &mut items_row);
            }
        }
    }
    flush_small_col(&mut small_buf, &mut items_row);

    let tools_el = items_row
        .into_iter()
        .fold(row![].spacing(2).height(Fill).align_y(iced::Top), |r, e| {
            r.push(e)
        });

    column![
        tools_el,
        container(text(group.title).size(9).color(GROUP_LABEL)).padding([1, 4]),
    ]
    .align_x(iced::Center)
    .spacing(0)
    .padding([3u16, 4])
    .height(Length::Fixed(TOOL_BAR_H))
    .into()
}

/// The top-level command id of a ribbon item, if it has one.
fn item_id(it: &RibbonItem) -> Option<&'static str> {
    match it {
        RibbonItem::Tool(t) | RibbonItem::LargeTool(t) => Some(t.id),
        RibbonItem::Dropdown { id, .. } | RibbonItem::LargeDropdown { id, .. } => Some(*id),
        RibbonItem::PropertiesGroup { match_prop } => Some(match_prop.id),
        _ => None,
    }
}

/// The tool a collapsed panel shows on its button: the last-used one, else the
/// panel's first tool-like item.
fn representative<'g>(group: &'g RibbonGroup, last_used: Option<&str>) -> Option<&'g RibbonItem> {
    if let Some(want) = last_used {
        if let Some(found) = group
            .tools
            .iter()
            .find(|&it| item_id(it).map_or(false, |id| id == want))
        {
            return Some(found);
        }
    }
    group.tools.iter().find(|&it| item_id(it).is_some())
}

/// The icon of a panel's first tool-like item. Shown on the tightest collapse
/// button; digs into the composite groups so Layers/Properties/Styles panels
/// still get a representative icon.
fn first_tool_icon(group: &RibbonGroup) -> Option<IconKind> {
    group.tools.iter().find_map(|it| match it {
        RibbonItem::Tool(t) | RibbonItem::LargeTool(t) => Some(t.icon),
        RibbonItem::Dropdown { icon, .. } | RibbonItem::LargeDropdown { icon, .. } => Some(*icon),
        RibbonItem::PropertiesGroup { match_prop } => Some(match_prop.icon),
        RibbonItem::LayerComboGroup { row2, .. } => row2.first().map(|t| t.icon),
        RibbonItem::StyleComboGroup { rows, .. } => {
            rows.first().and_then(|r| r.first()).map(|t| t.icon)
        }
    })
}

/// A collapsed panel. In the COLLAPSED step: a live representative-tool button
/// (updates to the last-used tool) plus a title + ▾ opener for the full flyout.
/// In the tighter `compact` (TIGHT) step: just the first tool's icon + ▾, a pure
/// flyout opener with no direct tool run. Height is natural content height
/// (shorter than a full 3-row panel), so `CollapsePanels` can shrink the ribbon.
#[allow(clippy::too_many_arguments)]
fn collapse_button<'a>(
    group: &RibbonGroup,
    last_used: Option<&str>,
    active_tool: &Option<String>,
    open_dd: &Option<String>,
    last_cmd: &HashMap<&'static str, &'static str>,
    state: widgets::ToggleState,
    layer_infos: &'a [LayerInfo],
    active_layer: &'a str,
    active_color: AcadColor,
    active_linetype: &'a str,
    active_lineweight: LineWeight,
    style_ctx: &StyleContext,
    // When set, this is the TIGHT step (reached once even the all-collapsed row
    // overflows): the whole panel becomes a single non-running button — the first
    // tool's icon + a ▾ that opens the tools flyout. Nothing runs directly here.
    compact: bool,
) -> Element<'a, Message> {
    let title = group.title;

    // Tightest form: one button = the panel's FIRST tool icon + its title + ▾.
    // Clicking opens the flyout listing every tool; no tool runs directly at this
    // density (you pick one from the dropdown), so the tightest row is pure openers.
    if compact {
        let icon: Element<'_, Message> = match first_tool_icon(group) {
            Some(ik) => make_icon(ik, SMALL_ICON),
            None => text("").into(),
        };
        return button(
            column![
                icon,
                row![
                    text(title.to_string()).size(9).color(GROUP_LABEL),
                    crate::ui::icons::arrow_down(8.0, GROUP_LABEL),
                ]
                .spacing(3)
                .align_y(iced::Center),
            ]
            .align_x(iced::Center)
            .spacing(2),
        )
        .on_press(Message::ToggleRibbonPanel(title.to_string()))
        .style(|_: &Theme, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => Color {
                    r: 0.25,
                    g: 0.25,
                    b: 0.25,
                    a: 1.0,
                },
                _ => Color::TRANSPARENT,
            })),
            border: Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .padding([3, 5])
        .into();
    }

    // Collapsed (not yet tight): a large representative-tool face — a live button
    // that runs the last-used tool — above a title + ▾ opener for the full flyout.
    // For a Properties panel the representative is its Match button.
    let rep = representative(group, last_used);
    let face: Element<'_, Message> = match rep {
        Some(RibbonItem::PropertiesGroup { match_prop }) => render_large(
            &RibbonItem::LargeTool(match_prop.clone()),
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
            false,
        ),
        Some(item) => render_large(
            item,
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
            false,
        ),
        None => text("").into(),
    };

    let opener = button(
        row![
            text(title.to_string()).size(9).color(GROUP_LABEL),
            crate::ui::icons::arrow_down(8.0, GROUP_LABEL),
        ]
        .spacing(3)
        .align_y(iced::Center),
    )
    .on_press(Message::ToggleRibbonPanel(title.to_string()))
    .style(|_: &Theme, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered => Color {
                r: 0.25,
                g: 0.25,
                b: 0.25,
                a: 1.0,
            },
            _ => Color::TRANSPARENT,
        })),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .padding([1, 4]);

    // The large face fills a fixed slot so a collapsed panel is shorter than a full
    // 3-row panel, letting `CollapsePanels` shrink the ribbon row.
    let face_box = container(face)
        .height(Length::Fixed(COLLAPSED_FACE_H))
        .align_y(iced::Center);

    column![face_box, opener]
        .align_x(iced::Center)
        .spacing(2)
        .padding([3u16, 4])
        .into()
}

impl Default for Ribbon {
    fn default() -> Self {
        Self::new()
    }
}
