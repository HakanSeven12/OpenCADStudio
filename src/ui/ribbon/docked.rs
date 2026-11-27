//! Ribbon panels docked as one-column toolbars at the window's left and
//! right edges (`RibbonLayout::docked`), beside the drawing.

use iced::widget::{button, column, container, tooltip, Space};
use iced::{Background, Border, Element, Length, Point, Theme};

use super::extras;
use super::layout::{DockedToolbar, ToolbarEdge};
use super::Ribbon;
use crate::app::Message;
use crate::modules::{ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

/// How near the window's side a floating panel is dropped to dock there.
pub(super) const DOCK_DROP_MARGIN: f32 = 24.0;

/// The id a docked toolbar reports its bounds under.
fn docked_id(key: &str) -> String {
    format!("ribbon-docked:{key}")
}

/// The buttons of `group` in one column: its tools, a dropdown as its
/// current item, without the combos that need a ribbon's width.
fn column_tools(
    group: &RibbonGroup,
    last_cmd: &rustc_hash::FxHashMap<&'static str, &'static str>,
) -> Vec<ToolDef> {
    let mut tools = Vec::new();
    for item in &group.tools {
        match item {
            RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) | RibbonItem::LargeTool(t) => {
                tools.push(t.clone())
            }
            RibbonItem::Dropdown {
                id, items, default, ..
            }
            | RibbonItem::LabeledDropdown {
                id, items, default, ..
            }
            | RibbonItem::LargeDropdown {
                id, items, default, ..
            } => {
                let face = last_cmd.get(id).copied().unwrap_or(*default);
                if let Some(&(cmd, label, icon)) = items
                    .iter()
                    .find(|(cmd, ..)| *cmd == face)
                    .or(items.first())
                {
                    tools.push(ToolDef {
                        id: cmd,
                        label,
                        icon,
                        event: ModuleEvent::Command(cmd.to_string()),
                    });
                }
            }
            RibbonItem::ToolGrid { columns } | RibbonItem::StyleComboGroup { rows: columns, .. } => {
                tools.extend(columns.iter().flatten().cloned())
            }
            RibbonItem::LayerComboGroup { row2, row3 } => {
                tools.extend(row2.iter().chain(row3).cloned())
            }
            RibbonItem::PropertiesGroup { match_prop } => tools.push(match_prop.clone()),
        }
    }
    tools
}

impl Ribbon {
    /// The docked toolbars with their group, in `layout.docked` order. One
    /// whose tab or group is gone is left out.
    fn docked_groups(&self) -> Vec<(&RibbonGroup, &DockedToolbar)> {
        self.layout
            .docked
            .iter()
            .filter_map(|toolbar| {
                let index = self
                    .modules
                    .iter()
                    .position(|module| module.id() == toolbar.module_id)?;
                let group = self.ordered_groups(index).into_iter().find(|group| {
                    extras::panel_key(&toolbar.module_id, group.title) == toolbar.key
                })?;
                Some((group, toolbar))
            })
            .collect()
    }

    /// Dock panel `title` of tab `module` at `edge`, out of its ribbon row
    /// and out of the floating layer. Whether anything changed.
    pub fn dock_toolbar(&mut self, module: &str, title: &str, edge: ToolbarEdge) -> bool {
        let Some(index) = self.modules.iter().position(|m| m.id() == module) else {
            return false;
        };
        let known = self
            .ordered_groups(index)
            .iter()
            .any(|group| group.title == title);
        if !known || self.is_contextual(module) {
            return false;
        }
        let key = extras::panel_key(module, title);
        let was_floating = self.dock_floating(&key);
        if let Some(toolbar) = self.layout.docked.iter_mut().find(|t| t.key == key) {
            let moved = toolbar.edge != edge;
            toolbar.edge = edge;
            return moved || was_floating;
        }
        self.layout.docked.push(DockedToolbar {
            key,
            module_id: module.to_string(),
            edge,
        });
        true
    }

    /// Take docked toolbar `key` off its edge; whether it was docked.
    pub(super) fn undock_toolbar(&mut self, key: &str) -> bool {
        let before = self.layout.docked.len();
        self.layout.docked.retain(|toolbar| toolbar.key != key);
        self.layout.docked.len() != before
    }

    /// Float docked toolbar `key` beside the place it was docked.
    pub(super) fn float_docked(&mut self, key: &str) -> bool {
        let Some(toolbar) = self.layout.docked.iter().find(|t| t.key == key).cloned() else {
            return false;
        };
        let (x, y) = crate::ui::wrap_bar::recent_bounds(&docked_id(key)).map_or(
            (60.0, 120.0),
            |b| match toolbar.edge {
                ToolbarEdge::Left => (b.x + b.width + 8.0, b.y),
                ToolbarEdge::Right => (b.x - 268.0, b.y),
            },
        );
        self.undock_toolbar(key);
        let title = key.split_once('/').map_or(key, |(_, title)| title).to_string();
        self.float_panel(&toolbar.module_id, &title, x.max(0.0), y.max(0.0));
        true
    }

    /// The edge a floating panel dropped at `at` docks on, if any.
    pub(super) fn drop_edge(&self, at: Point) -> Option<ToolbarEdge> {
        if at.x <= DOCK_DROP_MARGIN {
            Some(ToolbarEdge::Left)
        } else if at.x >= self.win_w - DOCK_DROP_MARGIN {
            Some(ToolbarEdge::Right)
        } else {
            None
        }
    }

    /// The toolbars docked at `edge`, one under the other in one band, each a
    /// column of buttons under a grip that floats it again. `None` when
    /// none is docked there.
    pub fn docked_toolbars(&self, edge: ToolbarEdge) -> Option<Element<'_, Message>> {
        let tip = match edge {
            ToolbarEdge::Left => tooltip::Position::Right,
            ToolbarEdge::Right => tooltip::Position::Left,
        };
        let bars: Vec<Element<'_, Message>> = self
            .docked_groups()
            .into_iter()
            .filter(|(_, toolbar)| toolbar.edge == edge)
            .map(|(group, toolbar)| {
                let grip = button(Space::new().width(Length::Fill).height(4.0))
                    .padding([3, 4])
                    .width(Length::Fill)
                    .on_press(Message::RibbonFloat(super::FloatMsg::Undock(
                        toolbar.key.clone(),
                    )))
                    .style(|theme: &Theme, status| {
                        let palette = theme.palette();
                        let hovered =
                            matches!(status, button::Status::Hovered | button::Status::Pressed);
                        button::Style {
                            background: Some(Background::Color(if hovered {
                                palette.primary.weak.color
                            } else {
                                palette.background.strong.color
                            })),
                            border: Border {
                                radius: 2.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    });
                let grip = tooltip(
                    grip,
                    container(iced::widget::text(crate::t!(group.title).into_owned()).size(11))
                        .padding([2, 6])
                        .style(container::bordered_box),
                    tip,
                );
                let mut col = column![grip].spacing(2).width(32.0);
                for tool in column_tools(group, &self.last_cmd) {
                    col = col.push(crate::ui::side_toolbar::compact_button(&tool, tip));
                }
                crate::ui::wrap_bar::PosReport::owned(docked_id(&toolbar.key), col).into()
            })
            .collect();
        if bars.is_empty() {
            return None;
        }
        // ponytail: a column taller than the window is cut off; wrap into a
        // second column if long panels get docked.
        Some(
            container(iced::widget::column(bars).spacing(6))
                .padding(2)
                .height(Length::Fill)
                .clip(true)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(theme.palette().background.weak.color)),
                    ..Default::default()
                })
                .into(),
        )
    }
}
