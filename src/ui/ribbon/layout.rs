//! The user's ribbon layout: hidden tabs and panels, the panel order, and the
//! minimize and slide-out preferences. Persisted in `RibbonConfig`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::modules::{CadModule, RibbonGroup};

/// How far the ribbon is minimized.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum RibbonMinimize {
    /// The full ribbon.
    #[default]
    Off,
    /// Only the tab row.
    Tabs,
    /// The tab row and a row of panel titles.
    PanelTitles,
    /// The tab row and a row of one button per panel.
    PanelButtons,
}

/// What the minimize button does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum MinimizePref {
    /// Step through every minimize state.
    #[default]
    Cycle,
    /// Switch between the full ribbon and the tab row.
    Tabs,
    /// Switch between the full ribbon and the panel titles.
    PanelTitles,
    /// Switch between the full ribbon and the panel buttons.
    PanelButtons,
}

/// The ribbon's persisted layout. Panels are named by their panel key
/// (`extras::panel_key`), tabs by their module id.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RibbonLayout {
    /// Module ids of the tabs the user hid.
    pub hidden_tabs: BTreeSet<String>,
    /// Panel keys of the panels the user hid.
    pub hidden_panels: BTreeSet<String>,
    /// Per module id, its panel keys in the user's order.
    pub panel_order: BTreeMap<String, Vec<String>>,
    pub show_panel_titles: bool,
    pub minimize: RibbonMinimize,
    pub minimize_pref: MinimizePref,
    /// Panel keys of the slide-outs pinned open.
    pub pinned_slideouts: BTreeSet<String>,
}

impl Default for RibbonLayout {
    fn default() -> Self {
        Self {
            hidden_tabs: BTreeSet::new(),
            hidden_panels: BTreeSet::new(),
            panel_order: BTreeMap::new(),
            show_panel_titles: true,
            minimize: RibbonMinimize::default(),
            minimize_pref: MinimizePref::default(),
            pinned_slideouts: BTreeSet::new(),
        }
    }
}

/// Whether `key` is the panel key of the panel `title` on module `module_id`.
fn is_key(key: &str, module_id: &str, title: &str) -> bool {
    key.strip_prefix(module_id)
        .and_then(|rest| rest.strip_prefix('/'))
        == Some(title)
}

impl RibbonLayout {
    /// `groups` of module `module_id` in the user's order. The panels the
    /// order lists take the places the listed panels have by default, sorted
    /// as listed; a panel it does not list (a new one) keeps its place.
    pub(super) fn order<'g>(
        &self,
        module_id: &str,
        mut groups: Vec<&'g RibbonGroup>,
    ) -> Vec<&'g RibbonGroup> {
        let Some(order) = self.panel_order.get(module_id) else {
            return groups;
        };
        let rank = |group: &RibbonGroup| {
            order
                .iter()
                .position(|key| is_key(key, module_id, group.title))
        };
        let slots: Vec<usize> = (0..groups.len())
            .filter(|&i| rank(groups[i]).is_some())
            .collect();
        let mut listed: Vec<&RibbonGroup> = slots.iter().map(|&i| groups[i]).collect();
        listed.sort_by_key(|group| rank(group));
        for (slot, group) in slots.into_iter().zip(listed) {
            groups[slot] = group;
        }
        groups
    }

    /// Back to the default panel order, every tab and panel shown and panel
    /// titles on. The minimize state and pinned slide-outs stay.
    pub(super) fn reset(&mut self) {
        self.hidden_tabs.clear();
        self.hidden_panels.clear();
        self.panel_order.clear();
        self.show_panel_titles = true;
    }

    pub(super) fn panel_hidden(&self, module_id: &str, title: &str) -> bool {
        self.hidden_panels
            .iter()
            .any(|key| is_key(key, module_id, title))
    }

    /// Whether `modules` show as they do by default: no tab or panel of
    /// theirs hidden, every panel in its default place and panel titles on.
    /// Entries naming a tab or panel that no longer exists do not count.
    pub(super) fn is_default(&self, modules: &[Box<dyn CadModule>]) -> bool {
        self.show_panel_titles
            && modules.iter().all(|module| {
                let id = module.id();
                let groups: Vec<&RibbonGroup> = module.ribbon_groups().iter().collect();
                !self.hidden_tabs.contains(id)
                    && groups
                        .iter()
                        .all(|group| !self.panel_hidden(id, group.title))
                    && self
                        .order(id, groups.clone())
                        .iter()
                        .zip(&groups)
                        .all(|(a, b)| std::ptr::eq(*a, *b))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(title: &'static str) -> RibbonGroup {
        RibbonGroup {
            title,
            tools: vec![],
        }
    }

    fn titles(groups: &[&RibbonGroup]) -> Vec<&'static str> {
        groups.iter().map(|group| group.title).collect()
    }

    struct Tab(Vec<RibbonGroup>);

    impl CadModule for Tab {
        fn id(&self) -> &'static str {
            "draw"
        }
        fn title(&self) -> &'static str {
            "Draw"
        }
        fn ribbon_groups(&self) -> &[RibbonGroup] {
            &self.0
        }
    }

    fn order_of(keys: &[&str]) -> RibbonLayout {
        let mut layout = RibbonLayout::default();
        layout.panel_order.insert(
            "draw".to_string(),
            keys.iter().map(|key| key.to_string()).collect(),
        );
        layout
    }

    #[test]
    fn the_order_sorts_the_panels_it_lists() {
        let (a, b, c) = (group("A"), group("B"), group("C"));
        let layout = order_of(&["draw/C", "draw/A", "draw/B"]);
        assert_eq!(
            titles(&layout.order("draw", vec![&a, &b, &c])),
            ["C", "A", "B"]
        );
        // Another tab's order does not apply.
        assert_eq!(
            titles(&layout.order("insert", vec![&a, &b, &c])),
            ["A", "B", "C"]
        );
    }

    #[test]
    fn a_panel_the_order_does_not_list_keeps_its_place() {
        let (a, n, b, c) = (group("A"), group("New"), group("B"), group("C"));
        let layout = order_of(&["draw/C", "draw/A", "draw/B", "draw/Gone"]);
        assert_eq!(
            titles(&layout.order("draw", vec![&a, &n, &b, &c])),
            ["C", "New", "A", "B"]
        );
    }

    #[test]
    fn a_hidden_panel_is_named_by_tab_and_title() {
        let mut layout = RibbonLayout::default();
        layout.hidden_panels.insert("draw/A".to_string());
        layout.hidden_panels.insert("insert/B".to_string());
        assert!(layout.panel_hidden("draw", "A"));
        assert!(!layout.panel_hidden("draw", "B"));
        assert!(!layout.panel_hidden("insert", "A"));
        assert!(!layout.panel_hidden("dra", "w/A"));
    }

    #[test]
    fn the_default_layout_ignores_stale_entries() {
        let modules: Vec<Box<dyn CadModule>> = vec![Box::new(Tab(vec![group("A"), group("B")]))];
        let mut layout = RibbonLayout::default();
        assert!(layout.is_default(&modules));

        // Naming panels and tabs that do not exist, or listing the default
        // order, changes nothing.
        layout.hidden_panels.insert("draw/Gone".to_string());
        layout.hidden_tabs.insert("gone".to_string());
        layout
            .panel_order
            .insert("draw".to_string(), vec!["draw/A".into(), "draw/B".into()]);
        assert!(layout.is_default(&modules));

        let mut moved = layout.clone();
        moved
            .panel_order
            .insert("draw".to_string(), vec!["draw/B".into(), "draw/A".into()]);
        assert!(!moved.is_default(&modules));

        let mut hidden = layout.clone();
        hidden.hidden_panels.insert("draw/B".to_string());
        assert!(!hidden.is_default(&modules));

        let mut hidden_tab = layout.clone();
        hidden_tab.hidden_tabs.insert("draw".to_string());
        assert!(!hidden_tab.is_default(&modules));

        let mut no_titles = layout;
        no_titles.show_panel_titles = false;
        assert!(!no_titles.is_default(&modules));
    }
}
