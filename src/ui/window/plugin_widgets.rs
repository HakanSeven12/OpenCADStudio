//! Native rendering of plugin widget surfaces (api v29, S05): trees and
//! lists rendered with the host's OWN widgets.
//!
//! Ownership split (the scene tier's law, applied to widgets): the plugin
//! owns the semantics — what rows mean; the host owns selection,
//! expansion, scroll and focus — all HOST-LOCAL transient state here, in
//! [`WidgetUiState`]. The one exception (finding F2's "unless the plugin
//! explicitly needs it"): expanding an `Unloaded` row notifies the plugin
//! (`SurfaceEvent::NodeExpanded`), because the plugin is the data source
//! for those children — the host cannot page them itself.
//!
//! The renderer walks the validated surface (bounds enforced by the
//! store) depth-first with a hard depth cap, so a hostile tree cannot
//! overflow the view-building stack. Clicks publish regular `Message`s —
//! no canvas, no hit-testing: native widgets route their own events.

use crate::app::Message;
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};
use ocs_plugin_api::scene::StableId;
use ocs_plugin_api::widget::{WidgetChildren, WidgetSurface};
use std::collections::HashSet;

/// Hard cap on render recursion depth (a validated surface can still be
/// deep; the view builder refuses to follow deeper chains).
const MAX_RENDER_DEPTH: usize = 64;

/// Host-local transient state for one panel's widget surface: which rows
/// are expanded and which row is selected. Nothing here is authoritative
/// about the DATA — the surface snapshot is; this is view state only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WidgetUiState {
    pub expanded: HashSet<StableId>,
    pub selected: Option<StableId>,
}

impl WidgetUiState {
    /// Toggle expansion. Returns `true` when the host must notify the
    /// plugin (`NodeExpanded`): only expanding an `Unloaded` row needs
    /// the plugin — it is the data source for those children.
    pub fn toggle_expanded(&mut self, node: StableId, children: &WidgetChildren) -> bool {
        if !self.expanded.remove(&node) {
            self.expanded.insert(node);
            return matches!(children, WidgetChildren::Unloaded);
        }
        false
    }

    /// Select a row (host-local highlight; no notification — selection
    /// stays host-local unless a later stage demonstrates a consumer).
    pub fn select(&mut self, node: StableId) {
        self.selected = Some(node);
    }
}

/// Render one widget surface with the host's native widgets. `owner`/
/// `panel` identify the surface for the event messages; `ui` is the
/// caller's host-local state (borrowed — the dispatch mutates it).
pub(crate) fn widget_view<'a>(
    owner: &'a str,
    panel: &'a str,
    surface: &WidgetSurface,
    ui: Option<&WidgetUiState>,
) -> Element<'a, Message> {
    // Panels render before their first interaction has created view state:
    // a missing entry is the empty host-local state, never an error.
    let default_ui = WidgetUiState::default();
    let ui = ui.unwrap_or(&default_ui);
    let list = render_level(surface, ui, &surface.roots.clone(), 0, owner, panel);
    scrollable(container(list.width(Length::Fill))).into()
}

fn render_level<'a>(
    surface: &WidgetSurface,
    ui: &WidgetUiState,
    ids: &[StableId],
    depth: usize,
    owner: &'a str,
    panel: &'a str,
) -> iced::widget::Column<'a, Message> {
    let mut column = column![].spacing(2);
    if depth > MAX_RENDER_DEPTH {
        return column;
    }
    for id in ids {
        let Some(row) = surface.rows.get(id) else {
            continue;
        };
        let expanded = ui.expanded.contains(id);
        let selected = ui.selected == Some(*id);
        let (marker, has_children) = match &row.children {
            WidgetChildren::Leaf => ("", false),
            WidgetChildren::Unloaded if expanded => ("▾", true),
            WidgetChildren::Unloaded => ("▸", true),
            WidgetChildren::Loaded(children) => {
                if children.is_empty() {
                    ("", false)
                } else if expanded {
                    ("▾", true)
                } else {
                    ("▸", true)
                }
            }
        };
        let marker_text = marker.to_owned();
        let row_id = *id;
        let expand_btn: Element<'a, Message> = if has_children {
            button(text(marker_text).size(12))
                .on_press(Message::PanelWidgetExpandToggled {
                    owner: owner.to_owned(),
                    panel: panel.to_owned(),
                    node: row_id,
                })
                .into()
        } else {
            text(" ").size(12).into()
        };
        let label = row.label.clone();
        let is_action = row.action.is_some();
        let activate_btn = button(row![text(label).size(13)].spacing(4))
            .on_press(Message::PanelWidgetNodeActivated {
                owner: owner.to_owned(),
                panel: panel.to_owned(),
                node: row_id,
            })
            .style(move |_, status| {
                // A button row has a border.
                let mut style = iced::widget::button::Style::default();
                if is_action {
                    style.border = iced::Border {
                        color: iced::Color::from_rgba8(150, 150, 190, 1.0),
                        width: 1.0,
                        radius: 3.0.into(),
                    };
                }
                if selected && status != iced::widget::button::Status::Disabled {
                    style.background = Some(iced::Background::Color(iced::Color::from_rgba8(
                        120, 120, 160, 0.25,
                    )));
                }
                style
            });
        column = column.push(
            row![expand_btn, container(activate_btn).width(Length::Fill)]
                .spacing(4)
                .width(Length::Fill),
        );
        // Loaded children render under the row when expanded (host-local).
        if expanded {
            if let WidgetChildren::Loaded(children) = &row.children {
                if depth + 1 <= MAX_RENDER_DEPTH {
                    let indent =
                        container(render_level(surface, ui, children, depth + 1, owner, panel))
                            .padding(iced::Padding {
                                top: 0.0,
                                right: 0.0,
                                bottom: 0.0,
                                left: 18.0,
                            });
                    column = column.push(indent);
                }
            }
            // Unloaded + expanded: children arrive when the plugin pushes
            // its page (the dispatch already notified NodeExpanded at
            // toggle time). Until then the row simply shows none.
        }
    }
    column
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::widget::{WidgetRevision, WidgetRow, WidgetSurface};

    fn surface_with_unloaded_parent() -> WidgetSurface {
        let mut surface = WidgetSurface::new(WidgetRevision(1));
        surface
            .rows
            .insert(StableId(1), row("parent", WidgetChildren::Unloaded));
        surface.roots = vec![StableId(1)];
        surface
    }

    fn row(label: &str, children: WidgetChildren) -> WidgetRow {
        WidgetRow {
            label: label.to_owned(),
            children,
            action: None,
        }
    }

    #[test]
    fn expanding_an_unloaded_row_requests_a_page_exactly_once() {
        let mut ui = WidgetUiState::default();
        assert!(
            ui.toggle_expanded(StableId(1), &WidgetChildren::Unloaded),
            "first expand of Unloaded must notify the plugin"
        );
        assert!(
            !ui.toggle_expanded(StableId(1), &WidgetChildren::Unloaded),
            "collapse must NOT notify"
        );
        assert!(
            ui.toggle_expanded(StableId(1), &WidgetChildren::Unloaded),
            "re-expanding an Unloaded row notifies again (data may have changed)"
        );
    }

    #[test]
    fn loaded_and_leaf_rows_never_request_pages() {
        let mut ui = WidgetUiState::default();
        assert!(!ui.toggle_expanded(StableId(1), &WidgetChildren::Leaf));
        assert!(!ui.toggle_expanded(StableId(2), &WidgetChildren::Loaded(vec![StableId(3)])));
    }

    #[test]
    fn selection_is_host_local_and_single() {
        let mut ui = WidgetUiState::default();
        ui.select(StableId(1));
        ui.select(StableId(2));
        assert_eq!(ui.selected, Some(StableId(2)), "single selection");
    }

    #[test]
    fn expansion_state_is_host_local_and_idempotent() {
        let mut ui = WidgetUiState::default();
        let children = WidgetChildren::Loaded(vec![StableId(3)]);
        // A loaded row expands on the host alone: no notification.
        assert!(!ui.toggle_expanded(StableId(1), &children));
        assert!(ui.expanded.contains(&StableId(1)));
        assert!(!ui.toggle_expanded(StableId(1), &children));
        assert!(!ui.expanded.contains(&StableId(1)));
    }
}
