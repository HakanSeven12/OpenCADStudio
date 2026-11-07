//! Dockable Tool Palettes panel (`TOOLPALETTES` / `TP`, `TOOLPALETTESCLOSE`).
//!
//! Provides reusable block tools organized into palette tabs, integrated with
//! the shared docking system (`PanelId::ToolPalettes`) and backed by the
//! existing block capture/import, insertion, selection, persistence, and
//! undo/redo services.

use crate::app::settings::BlockInsertOptions;
use crate::app::{ClipboardDeps, Message};
use crate::ui::dock::{self, Chrome, PanelId, VerticalLabel};
use crate::ui::ribbon::{tooltip_content, tooltip_style};
use crate::ui::window::block_palette::{preview_canvas, BlockPreview};
use iced::widget::{
    button, canvas, checkbox, column, container, pick_list, row, rule, scrollable, text,
    text_input, tooltip, Space,
};
use iced::{Background, Border, Element, Fill, Length, Theme};
use serde::{Deserialize, Serialize};
use std::rc::Rc;

/// A reusable block tool entry on a tool palette tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BlockTool {
    /// Canonical block definition name (`BlockRecord::name`).
    pub name: String,
    /// Display label shown on the tool row.
    pub label: String,
    /// Optional description shown in tooltip.
    pub description: String,
    /// Source drawing path if known (empty when created from an unsaved drawing).
    pub source: String,
    /// Whether the captured block definition is a dynamic block (`BlockGraph`).
    pub dynamic: bool,
    /// Whether the block definition is annotative.
    pub annotative: bool,
    /// Uniform scale factor applied when inserting this tool.
    pub scale: f64,
    /// Captured signed axis scales; absent for older saved tools.
    pub scale_xyz: Option<[f64; 3]>,
    /// Rotation angle in degrees applied when inserting this tool.
    pub rotation_deg: f64,
    /// Whether to explode the block reference upon insertion.
    pub explode: bool,
    /// Self-contained snapshot of the block definition, nested blocks,
    /// dynamic `BlockGraph` (parameters/actions), and table dependencies so the
    /// tool can be inserted into any drawing tab and across sessions.
    pub captured: Option<ClipboardDeps>,
    /// Cached wireframe preview for the palette thumbnail.
    #[serde(skip)]
    pub preview: Rc<BlockPreview>,
}

impl Default for BlockTool {
    fn default() -> Self {
        Self {
            name: String::new(),
            label: String::new(),
            description: String::new(),
            source: String::new(),
            dynamic: false,
            annotative: false,
            scale: 1.0,
            scale_xyz: None,
            rotation_deg: 0.0,
            explode: false,
            captured: None,
            preview: Rc::new(BlockPreview::default()),
        }
    }
}

impl PartialEq for BlockTool {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.label == other.label
            && self.description == other.description
            && self.source == other.source
            && self.dynamic == other.dynamic
            && self.annotative == other.annotative
            && self.scale == other.scale
            && self.scale_xyz == other.scale_xyz
            && self.rotation_deg == other.rotation_deg
            && self.explode == other.explode
            && self.captured == other.captured
    }
}

/// One named tab in the Tool Palettes panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolPaletteTab {
    pub name: String,
    pub tools: Vec<BlockTool>,
}

impl Default for ToolPaletteTab {
    fn default() -> Self {
        Self {
            name: crate::tr!("tool-palettes", "default-tab"),
            tools: Vec::new(),
        }
    }
}

/// Persisted Tool Palettes state stored in `UserSettings` / `AppConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolPalettesData {
    pub palettes: Vec<ToolPaletteTab>,
    pub active_palette: usize,
    pub options: BlockInsertOptions,
}

impl Default for ToolPalettesData {
    fn default() -> Self {
        Self {
            palettes: vec![ToolPaletteTab::default()],
            active_palette: 0,
            options: BlockInsertOptions::default(),
        }
    }
}

/// Runtime state of the Tool Palettes dockable panel.
#[derive(Debug, Clone)]
pub struct ToolPalettesState {
    pub palettes: Vec<ToolPaletteTab>,
    pub active_palette: usize,
    pub search: String,
    pub selected_drawing_block: Option<String>,
    pub new_palette_name: String,
    pub options: BlockInsertOptions,
    /// Name of the block tool currently being placed interactively, if any.
    pub placing: Option<String>,
}

impl Default for ToolPalettesState {
    fn default() -> Self {
        let data = ToolPalettesData::default();
        Self {
            palettes: data.palettes,
            active_palette: data.active_palette,
            search: String::new(),
            selected_drawing_block: None,
            new_palette_name: String::new(),
            options: data.options,
            placing: None,
        }
    }
}

impl ToolPalettesState {
    pub fn ensure_non_empty(&mut self) {
        if self.palettes.is_empty() {
            self.palettes.push(ToolPaletteTab::default());
        }
        if self.active_palette >= self.palettes.len() {
            self.active_palette = self.palettes.len() - 1;
        }
    }

    pub fn active_tab(&self) -> &ToolPaletteTab {
        let idx = self
            .active_palette
            .min(self.palettes.len().saturating_sub(1));
        &self.palettes[idx]
    }

    pub fn active_tab_mut(&mut self) -> &mut ToolPaletteTab {
        self.ensure_non_empty();
        let idx = self.active_palette;
        &mut self.palettes[idx]
    }

    pub fn to_persisted(&self) -> ToolPalettesData {
        ToolPalettesData {
            palettes: self.palettes.clone(),
            active_palette: self.active_palette,
            options: self.options.clone(),
        }
    }

    pub fn apply_persisted(&mut self, data: ToolPalettesData) {
        self.palettes = data.palettes;
        self.active_palette = data.active_palette;
        self.options = data.options;
        self.ensure_non_empty();
    }
}

#[derive(Debug, Clone)]
pub enum ToolPalettesMsg {
    SelectPalette(usize),
    NewPalette,
    NewPaletteNameChanged(String),
    DeletePalette(usize),
    SearchChanged(String),
    SelectDrawingBlock(String),
    AddDrawingBlock(String),
    AddFromSelection,
    InsertTool(usize),
    DropTool(usize, glam::DVec3),
    RemoveTool(usize),
    MoveToolUp(usize),
    MoveToolDown(usize),
    TogglePromptInsertionPoint(bool),
    ToggleExplode(bool),
}

fn tm(msg: ToolPalettesMsg) -> Message {
    Message::ToolPalettes(msg)
}

/// User-defined block names in `document` eligible for adding to a tool palette.
pub fn user_block_names(doc: &document::Document) -> Vec<String> {
    let mut names: Vec<String> = doc
        .block_records
        .iter()
        .filter(|br| {
            !br.name.starts_with('*')
                && !br.is_anonymous()
                && !br.flags.is_xref
                && !br.name.contains('|')
                && !br.name.eq_ignore_ascii_case("*Model_Space")
                && !br.name.eq_ignore_ascii_case("*Paper_Space")
        })
        .map(|br| br.name.clone())
        .collect();
    names.sort_by_key(|a| a.to_ascii_lowercase());
    names
}

/// Render the dockable Tool Palettes panel.
pub fn view<'a>(
    state: &'a ToolPalettesState,
    doc: &'a document::Document,
    selected: &'a rustc_hash::FxHashSet<document::Handle>,
    width: f32,
    chrome: Chrome,
) -> Element<'a, Message> {
    let has_selected_insert = selected.iter().any(|h| {
        matches!(
            doc.get_entity(*h),
            Some(document::EntityType::Insert(_))
        )
    });

    let available_blocks = user_block_names(doc);
    let chosen_block = state
        .selected_drawing_block
        .as_ref()
        .filter(|name| available_blocks.iter().any(|b| b == *name))
        .cloned()
        .or_else(|| available_blocks.first().cloned());

    // Top controls: filter + add from selection + add from current drawing
    let search_bar = text_input(
        &crate::tr!("tool-palettes", "search-placeholder"),
        &state.search,
    )
    .on_input(|s| tm(ToolPalettesMsg::SearchChanged(s)))
    .size(12)
    .padding([4, 6])
    .width(Fill);

    let add_selected_btn = {
        let btn = button(
            text(crate::tr!("tool-palettes", "add-selected"))
                .size(11)
                .center(),
        )
        .padding([4, 8])
        .width(Fill)
        .on_press_maybe(has_selected_insert.then_some(tm(ToolPalettesMsg::AddFromSelection)));
        tooltip(
            container(btn).id("tool-palettes-add-selected"),
            tooltip_content(crate::tr!("tool-palettes", "add-selected-hint")),
            tooltip::Position::Bottom,
        )
        .gap(4.0)
        .style(tooltip_style)
    };

    let add_drawing_row: Element<'a, Message> = if available_blocks.is_empty() {
        Space::new().height(0).into()
    } else {
        let picker = pick_list(
            chosen_block.clone(),
            available_blocks,
            |name: &String| name.clone(),
        )
        .on_select(|name| tm(ToolPalettesMsg::SelectDrawingBlock(name)))
        .text_size(11)
        .padding([3, 6])
        .width(Fill);

        let add_btn = button(text("+").size(12).center())
            .padding([3, 8])
            .on_press_maybe(chosen_block.map(|n| tm(ToolPalettesMsg::AddDrawingBlock(n))));

        row![picker, container(add_btn).id("tool-palettes-add-drawing-block")]
            .spacing(4)
            .align_y(iced::Center)
            .into()
    };

    // Palette tab management bar (new palette input + button)
    let new_tab_row = row![
        text_input(
            &crate::tr!("tool-palettes", "new-palette"),
            &state.new_palette_name,
        )
        .on_input(|s| tm(ToolPalettesMsg::NewPaletteNameChanged(s)))
        .on_submit(tm(ToolPalettesMsg::NewPalette))
        .size(11)
        .padding([3, 6])
        .width(Fill),
        button(text("+").size(12).center())
            .padding([3, 8])
            .on_press(tm(ToolPalettesMsg::NewPalette)),
        button(text("−").size(12).center())
            .padding([3, 8])
            .on_press_maybe(
                (state.palettes.len() > 1)
                    .then_some(tm(ToolPalettesMsg::DeletePalette(state.active_palette))),
            ),
    ]
    .spacing(4)
    .align_y(iced::Center);

    // Tool list for the active palette
    let pattern = state.search.trim();
    let filter = format!("*{pattern}*");
    let active_tab = state.active_tab();
    let mut tool_rows: Vec<Element<'a, Message>> = Vec::new();
    let total_tools = active_tab.tools.len();

    for (idx, tool) in active_tab.tools.iter().enumerate() {
        if !pattern.is_empty()
            && !crate::io::xref_model::wildcard_match(&tool.label, &filter)
            && !crate::io::xref_model::wildcard_match(&tool.name, &filter)
        {
            continue;
        }
        let is_placing = state.placing.as_deref() == Some(tool.name.as_str());
        let wires = tool.preview.wires().unwrap_or_default();
        let thumb: Element<'a, Message> = container(preview_canvas(wires, 36.0))
            .width(Length::Fixed(44.0))
            .height(Length::Fixed(36.0))
            .into();

        let mut badges = Vec::new();
        if tool.dynamic {
            badges.push(crate::tr!("tool-palettes", "dynamic-badge"));
        }
        if tool.annotative {
            badges.push(crate::tr!("tool-palettes", "annotative-badge"));
        }
        let subtitle = if badges.is_empty() {
            tool.name.clone()
        } else {
            format!("{} ({})", tool.name, badges.join(", "))
        };

        let main_btn = button(
            row![
                thumb,
                column![
                    text(tool.label.clone()).size(12),
                    text(subtitle).size(10),
                ]
                .spacing(2)
                .width(Fill),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .on_press(tm(ToolPalettesMsg::InsertTool(idx)))
        .padding([4, 6])
        .width(Fill)
        .style(move |theme: &Theme, status| {
            let mut s = button::text(theme, status);
            if is_placing {
                s.border = Border {
                    color: theme.palette().primary.base.color,
                    width: 1.0,
                    radius: 3.0.into(),
                };
            }
            s
        });

        let up_btn = button(text("↑").size(10).center())
            .padding([2, 5])
            .on_press_maybe((idx > 0).then_some(tm(ToolPalettesMsg::MoveToolUp(idx))));
        let down_btn = button(text("↓").size(10).center())
            .padding([2, 5])
            .on_press_maybe(
                (idx + 1 < total_tools).then_some(tm(ToolPalettesMsg::MoveToolDown(idx))),
            );
        let del_btn = button(text("×").size(11).center())
            .padding([2, 5])
            .on_press(tm(ToolPalettesMsg::RemoveTool(idx)));

        let row_el = container(
            row![main_btn, up_btn, down_btn, del_btn]
                .spacing(2)
                .align_y(iced::Center),
        )
        .id(format!("tool-palette-item-{idx}"))
        .into();

        tool_rows.push(row_el);
    }

    let tools_list: Element<'a, Message> = if tool_rows.is_empty() {
        container(
            text(crate::tr!("tool-palettes", "no-tools"))
                .size(11)
                .color( iced::Color::from_rgb(0.6, 0.6, 0.6) ),
        )
        .padding(10)
        .width(Fill)
        .into()
    } else {
        scrollable(column(tool_rows).spacing(3).padding([2, 4]))
            .width(Fill)
            .height(Fill)
            .into()
    };

    // Vertical palette tabs along the right side (matching Authoring Palettes chrome)
    let side_tabs = column(state.palettes.iter().enumerate().map(|(idx, tab)| {
        let selected = idx == state.active_palette;
        let label = tab.name.clone();
        let tab_h = ((label.chars().count() as f32) * 7.0 + 12.0).max(40.0);
        container(
            button(
                canvas(VerticalLabel {
                    text: label,
                    centered: false,
                })
                .width(16)
                .height(tab_h),
            )
            .on_press(tm(ToolPalettesMsg::SelectPalette(idx)))
            .padding([6, 4])
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                button::Style {
                    background: Some(Background::Color(
                        if selected || status == button::Status::Hovered {
                            palette.background.base.color
                        } else {
                            palette.background.weak.color
                        },
                    )),
                    border: Border {
                        color: if selected {
                            palette.primary.base.color
                        } else {
                            palette.background.neutral.color
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }
            }),
        )
        .id(format!("tool-palette-tab-{idx}"))
        .into()
    }))
    .spacing(2)
    .push(Space::new().height(Fill));

    // Placement options footer
    let options_footer = column![
        rule::horizontal(1),
        checkbox(state.options.insertion_point)
            .label(crate::t!("Insertion point").into_owned())
            .on_toggle(|v| tm(ToolPalettesMsg::TogglePromptInsertionPoint(v)))
            .size(13)
            .text_size(11),
        checkbox(state.options.explode)
            .label(crate::t!("Explode").into_owned())
            .on_toggle(|v| tm(ToolPalettesMsg::ToggleExplode(v)))
            .size(13)
            .text_size(11),
    ]
    .spacing(4)
    .padding([4, 6]);

    let body = column![
        column![search_bar, add_selected_btn, add_drawing_row, new_tab_row]
            .spacing(4)
            .padding([6, 6]),
        rule::horizontal(1),
        row![tools_list, side_tabs].spacing(4).height(Fill),
        options_footer,
    ]
    .height(Fill);

    dock::frame(
        dock::title_bar(
            PanelId::ToolPalettes,
            crate::tr!("tool-palettes", "title"),
            chrome,
        ),
        body,
        width,
    )
}
