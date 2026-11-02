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

/// One item of the Quick Access Toolbar.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum QatItem {
    /// Any command line, e.g. "QSAVE", "HATCH", "ZOOM E", or the id of a
    /// ribbon dropdown (a split item whose face runs the last one chosen).
    Command(String),
    /// A built-in control with its own UI.
    Builtin(QatBuiltin),
    Separator,
}

impl QatItem {
    /// Whether `self` and `other` are the same item: commands ignore case
    /// and surrounding spaces.
    pub fn same(&self, other: &QatItem) -> bool {
        match (self, other) {
            (QatItem::Command(a), QatItem::Command(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
            _ => self == other,
        }
    }
}

/// The Quick Access Toolbar's controls with a UI of their own.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum QatBuiltin {
    Undo,
    Redo,
    LayerCombo,
    SwitchWindows,
    UserInterface,
    /// The ribbon's other lists (Home > Properties, the Annotate styles
    /// and layers), each opening the same list as on the ribbon.
    Color,
    Linetype,
    Lineweight,
    TextStyle,
    DimStyle,
    MleaderStyle,
    TableStyle,
    DimLayer,
    MleaderLayer,
    CenterLayer,
}

fn lenient_qat<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<QatItem>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Saved {
        Known(QatItem),
        Unknown(serde::de::IgnoredAny),
    }
    Ok(Vec::<Saved>::deserialize(deserializer)?
        .into_iter()
        .filter_map(|item| match item {
            Saved::Known(item) => Some(item),
            Saved::Unknown(_) => None,
        })
        .collect())
}

/// The toolbar a user with no saved one sees: the file commands, then Undo
/// and Redo.
pub fn default_qat() -> Vec<QatItem> {
    let command = |cmd: &str| QatItem::Command(cmd.to_string());
    vec![
        command("QNEW"),
        command("OPEN"),
        command("SAVE"),
        command("SAVEAS"),
        command("PRINT"),
        QatItem::Builtin(QatBuiltin::Undo),
        QatItem::Builtin(QatBuiltin::Redo),
    ]
}

/// A ribbon panel dragged out of the ribbon, floating over the drawing.
/// It keeps its slot in `panel_order`, so it can go back there.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FloatingPanel {
    /// Its panel key (`extras::panel_key`).
    pub key: String,
    /// The module id of the tab it belongs to.
    pub module_id: String,
    /// Its top-left corner in logical px, in the window's content area.
    pub x: f32,
    pub y: f32,
    /// Its title at the side and its extra tools opening to the right,
    /// instead of the title at the bottom and the tools opening downward.
    pub extras_right: bool,
    /// Its extra tools are pinned open.
    pub pinned: bool,
    /// Stacking order: higher is on top.
    pub z: u32,
}

impl Default for FloatingPanel {
    fn default() -> Self {
        Self {
            key: String::new(),
            module_id: String::new(),
            x: 0.0,
            y: 0.0,
            extras_right: false,
            pinned: false,
            z: 0,
        }
    }
}

impl FloatingPanel {
    /// Where the panel may be, `size` big (unknown until it is drawn), in a
    /// window `win` big, so that at least its title strip stays inside:
    /// the bottom strip, or the side strip with the title on the right.
    /// Returns (x range, y range).
    fn allowed(&self, size: (f32, f32), win: (f32, f32)) -> ((f32, f32), (f32, f32)) {
        use crate::ui::style::ribbon_metrics as metrics;
        let (w, h) = size;
        let range = |low: f32, high: f32| (low.min(high), high);
        if self.extras_right {
            (
                range(metrics::FLOAT_SIDE_STRIP_W - w, win.0 - w),
                range(0.0, win.1 - h),
            )
        } else {
            (
                range(0.0, win.0 - w),
                range(metrics::TITLE_STRIP_H - h, win.1 - h),
            )
        }
    }

    /// Move the panel, `size` big, so its title strip is inside a window
    /// `win` big.
    pub(super) fn clamp(&mut self, size: (f32, f32), win: (f32, f32)) {
        let ((x0, x1), (y0, y1)) = self.allowed(size, win);
        self.x = self.x.clamp(x0, x1.max(x0));
        self.y = self.y.clamp(y0, y1.max(y0));
    }
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
    /// The command search box is kept open; collapsed it shows as its
    /// magnifier button.
    pub search_expanded: bool,
    /// The Quick Access Toolbar, left to right. An item this build does not
    /// know (saved by a newer one) is dropped rather than failing the whole
    /// settings file.
    #[serde(deserialize_with = "lenient_qat")]
    pub qat: Vec<QatItem>,
    /// Show the Quick Access Toolbar in its own row below the ribbon.
    pub qat_below: bool,
    /// Per split item, the command its face runs (the last one chosen).
    pub qat_faces: BTreeMap<String, String>,
    /// The panels floating over the drawing.
    pub floating: Vec<FloatingPanel>,
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
            search_expanded: true,
            qat: default_qat(),
            qat_below: false,
            qat_faces: BTreeMap::new(),
            floating: Vec::new(),
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

    /// Back to the default panel order, every tab and panel shown in the
    /// ribbon (floating ones too) and panel titles on. The minimize state,
    /// pinned slide-outs and the Quick Access Toolbar stay.
    pub(super) fn reset(&mut self) {
        self.hidden_tabs.clear();
        self.hidden_panels.clear();
        self.panel_order.clear();
        self.show_panel_titles = true;
        self.floating.clear();
    }

    /// Whether the Quick Access Toolbar holds `item`.
    pub(super) fn qat_has(&self, item: &QatItem) -> bool {
        self.qat.iter().any(|held| held.same(item))
    }

    /// Add `item` at the end of the Quick Access Toolbar unless it is there
    /// already; whether it was added. Separators are always added.
    pub(super) fn qat_add(&mut self, item: QatItem) -> bool {
        if item != QatItem::Separator && self.qat_has(&item) {
            return false;
        }
        self.qat.push(item);
        true
    }

    /// Take `item` off the Quick Access Toolbar, with a separator it leaves
    /// at either end or doubled; whether it was there.
    pub(super) fn qat_remove(&mut self, item: &QatItem) -> bool {
        let before = self.qat.len();
        self.qat.retain(|held| !held.same(item));
        self.tidy_qat_separators();
        self.qat.len() != before
    }

    /// Add `item` if the toolbar lacks it, else take it off.
    pub(super) fn qat_toggle(&mut self, item: QatItem) {
        if !self.qat_remove(&item) {
            self.qat_add(item);
        }
    }

    /// Keep the first of any repeated Quick Access item (a hand-edited
    /// config may repeat one), so no two items share a key tip anchor.
    pub(super) fn dedupe_qat(&mut self) {
        let mut kept: Vec<QatItem> = Vec::with_capacity(self.qat.len());
        for item in self.qat.drain(..) {
            if item == QatItem::Separator || !kept.iter().any(|held| held.same(&item)) {
                kept.push(item);
            }
        }
        self.qat = kept;
        self.tidy_qat_separators();
    }

    /// Drop separators at the ends of the toolbar and doubled ones.
    pub(super) fn tidy_qat_separators(&mut self) {
        let mut tidy: Vec<QatItem> = Vec::with_capacity(self.qat.len());
        for item in self.qat.drain(..) {
            let is_separator = item == QatItem::Separator;
            if is_separator && tidy.last().is_none_or(|last| *last == QatItem::Separator) {
                continue;
            }
            tidy.push(item);
        }
        if tidy.last() == Some(&QatItem::Separator) {
            tidy.pop();
        }
        self.qat = tidy;
    }

    /// The panel `title` of module `module_id`, if it floats.
    pub(super) fn floating_panel(&self, module_id: &str, title: &str) -> Option<&FloatingPanel> {
        self.floating
            .iter()
            .find(|panel| is_key(&panel.key, module_id, title))
    }

    /// Keep each floating panel's title strip inside a window `win` big;
    /// `size_of` gives a panel's size by its key once it has been drawn.
    pub(super) fn clamp_floating(
        &mut self,
        win: (f32, f32),
        size_of: impl Fn(&str) -> Option<(f32, f32)>,
    ) {
        use crate::ui::style::ribbon_metrics as metrics;
        if !(win.0.is_finite() && win.1.is_finite()) {
            return;
        }
        for panel in &mut self.floating {
            // Before it is drawn, as small as its title strip.
            let size = size_of(&panel.key)
                .unwrap_or((metrics::FLOAT_SIDE_STRIP_W, metrics::TITLE_STRIP_H));
            panel.clamp(size, win);
        }
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
            && self.floating.is_empty()
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
    fn a_floating_panel_keeps_its_title_strip_in_the_window() {
        let win = (1000.0, 800.0);
        let size = (200.0, 99.0);
        let mut down = FloatingPanel {
            x: -50.0,
            y: 900.0,
            ..FloatingPanel::default()
        };
        down.clamp(size, win);
        assert_eq!((down.x, down.y), (0.0, 701.0));
        // Above the window, the strip at the bottom may stay just inside.
        down.y = -500.0;
        down.clamp(size, win);
        assert_eq!(down.y, 20.0 - 99.0);

        // With the title at the side, the panel may hang off the left edge
        // up to its side strip, but not off the top.
        let mut right = FloatingPanel {
            x: -500.0,
            y: -10.0,
            extras_right: true,
            ..FloatingPanel::default()
        };
        right.clamp(size, win);
        assert_eq!((right.x, right.y), (20.0 - 200.0, 0.0));
        right.x = 5000.0;
        right.clamp(size, win);
        assert_eq!(right.x, 800.0);
    }

    #[test]
    fn floating_panels_clamp_to_a_smaller_window_and_are_found_by_key() {
        let mut layout = RibbonLayout::default();
        layout.floating.push(FloatingPanel {
            key: "draw/Draw".into(),
            module_id: "draw".into(),
            x: 900.0,
            y: 500.0,
            ..FloatingPanel::default()
        });
        assert!(layout.floating_panel("draw", "Draw").is_some());
        assert!(layout.floating_panel("draw", "Modify").is_none());
        layout.clamp_floating((600.0, 400.0), |_| Some((300.0, 99.0)));
        let panel = &layout.floating[0];
        assert_eq!((panel.x, panel.y), (300.0, 301.0));
        // An unknown window size changes nothing.
        layout.clamp_floating((f32::INFINITY, f32::INFINITY), |_| None);
        assert_eq!(layout.floating[0].x, 300.0);
    }

    #[test]
    fn the_quick_access_toolbar_adds_once_and_removes_ignoring_case() {
        let mut layout = RibbonLayout::default();
        let hatch = QatItem::Command("HATCH".into());
        assert!(!layout.qat_has(&hatch));
        assert!(layout.qat_add(hatch.clone()));
        assert!(!layout.qat_add(QatItem::Command(" hatch ".into())));
        assert_eq!(layout.qat.last(), Some(&hatch));

        layout.qat_toggle(QatItem::Command("hatch".into()));
        assert!(!layout.qat_has(&hatch));
        layout.qat_toggle(QatItem::Builtin(QatBuiltin::LayerCombo));
        assert_eq!(
            layout.qat.last(),
            Some(&QatItem::Builtin(QatBuiltin::LayerCombo))
        );
    }

    #[test]
    fn removing_an_item_drops_the_separators_it_strands() {
        let mut layout = RibbonLayout::default();
        let save = QatItem::Command("SAVE".into());
        let print = QatItem::Command("PRINT".into());
        layout.qat = vec![
            save.clone(),
            QatItem::Separator,
            print.clone(),
            QatItem::Separator,
            QatItem::Builtin(QatBuiltin::Undo),
        ];
        layout.qat_remove(&print);
        assert_eq!(
            layout.qat,
            [
                save.clone(),
                QatItem::Separator,
                QatItem::Builtin(QatBuiltin::Undo)
            ]
        );
        layout.qat_remove(&save);
        assert_eq!(layout.qat, [QatItem::Builtin(QatBuiltin::Undo)]);
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
