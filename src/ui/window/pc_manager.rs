//! Point Cloud Manager (POINTCLOUDMANAGER): the regions and scans of the
//! drawing's point clouds as a tree, each with a switch that shows or hides
//! its points. The switches are the cloud's own hidden scans and regions, so
//! the tree is read straight from the document on every view.

use crate::app::Message;
use crate::scene::model::point_cloud::{self, UNASSIGNED_OFF};
use crate::ui::dock::PanelId;
use crate::ui::kit;
use crate::ui::style::{chrome, kit_metrics as km};
use document::entities::ExtendedEntityData;
use document::{Document, EntityType, Handle};
use iced::widget::{button, checkbox, column, container};
use iced::{Element, Fill, Length};
use std::collections::HashSet;

const CLOUD_ICON: &[u8] = include_bytes!("../../../assets/icons/pc_attach.svg");
const REGION_ICON: &[u8] = include_bytes!("../../../assets/icons/region.svg");
const UNASSIGNED_ICON: &[u8] = include_bytes!("../../../assets/icons/revcloud.svg");

/// Palette state that is not in the drawing: shown, folded nodes, the
/// highlighted row and the search text.
#[derive(Debug, Default)]
pub struct PcManager {
    pub show: bool,
    pub collapsed: HashSet<String>,
    pub selected: Option<String>,
    pub search: String,
}

/// A row with a switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Cloud,
    Unassigned,
    Scans,
    /// A scan, by identifier.
    Scan(String),
}

/// A switch's state: a parent is mixed while some children are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    On,
    Off,
    Mixed,
}

impl Row {
    /// `cloud`, `unassigned`, `scans` or `scan:<name or identifier>`
    /// (automation).
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "cloud" => Some(Self::Cloud),
            "unassigned" => Some(Self::Unassigned),
            "scans" => Some(Self::Scans),
            _ => text
                .strip_prefix("scan:")
                .map(|name| Self::Scan(name.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub enum PcManagerMsg {
    Toggle(Handle, Row),
    Expand(String),
    Select(String),
    Search(String),
    CollapseAll,
    ExpandAll,
}

/// One cloud of the tree.
pub struct Cloud {
    pub handle: Handle,
    pub name: String,
    pub hidden: Vec<String>,
    /// Each scan's name and identifier.
    pub scans: Vec<(String, String)>,
}

/// The drawing's point clouds; only the selected ones when any is selected.
pub fn clouds(document: &Document, selected: &[Handle]) -> Vec<Cloud> {
    let all: Vec<Cloud> = document
        .entities()
        .filter_map(|entity| {
            let EntityType::Extended(extended) = entity else {
                return None;
            };
            let ExtendedEntityData::PointCloudEx(data) = &extended.data else {
                return None;
            };
            let common = entity.common();
            let scans = point_cloud::resolve_source(document, data)
                .and_then(|path| point_cloud::cached(&path))
                .map(|cloud| {
                    cloud
                        .scan_ranges
                        .iter()
                        .map(|s| (s.name.clone(), s.id.clone()))
                        .collect()
                })
                .unwrap_or_default();
            Some(Cloud {
                handle: common.handle,
                name: if data.name.is_empty() {
                    format!("{:X}", common.handle.value())
                } else {
                    data.name.clone()
                },
                hidden: point_cloud::hidden(data),
                scans,
            })
        })
        .collect();
    if all.iter().any(|cloud| selected.contains(&cloud.handle)) {
        all.into_iter()
            .filter(|cloud| selected.contains(&cloud.handle))
            .collect()
    } else {
        all
    }
}

/// A row's switch. A parent is on with all its children on, off with all
/// off, mixed otherwise (the reference shows the cloud mixed with its only
/// scan off and its unassigned points on).
pub fn switch_state(hidden: &[String], scans: &[(String, String)], row: &Row) -> Switch {
    let on = |id: &str| !hidden.iter().any(|h| h == id);
    let of = |states: &[bool]| match (states.iter().all(|s| *s), states.iter().any(|s| *s)) {
        (true, _) => Switch::On,
        (false, false) => Switch::Off,
        _ => Switch::Mixed,
    };
    let scan_states: Vec<bool> = scans.iter().map(|(_, id)| on(id)).collect();
    match row {
        Row::Scan(id) => of(&[on(id)]),
        Row::Scans => of(&scan_states),
        Row::Unassigned => of(&[on(UNASSIGNED_OFF)]),
        Row::Cloud => {
            let mut all = scan_states;
            all.push(on(UNASSIGNED_OFF));
            of(&all)
        }
    }
}

/// The hidden list after clicking `row`: an on switch turns its rows off,
/// an off or mixed one turns them all on.
pub fn toggled(hidden: &[String], scans: &[(String, String)], row: &Row) -> Vec<String> {
    fn set(out: &mut Vec<String>, id: &str, off: bool) {
        out.retain(|h| h != id);
        if off {
            out.push(id.to_string());
        }
    }
    let off = switch_state(hidden, scans, row) == Switch::On;
    let mut out = hidden.to_vec();
    match row {
        Row::Cloud => {
            scans.iter().for_each(|(_, id)| set(&mut out, id, off));
            set(&mut out, UNASSIGNED_OFF, off);
        }
        Row::Unassigned => set(&mut out, UNASSIGNED_OFF, off),
        Row::Scans => scans.iter().for_each(|(_, id)| set(&mut out, id, off)),
        Row::Scan(id) => set(&mut out, id, off),
    }
    out
}

/// Keys of the rows that fold: each cloud and its Scans.
pub fn folding_keys(clouds: &[Cloud]) -> Vec<String> {
    clouds
        .iter()
        .flat_map(|c| [key(c.handle), format!("{}/scans", key(c.handle))])
        .collect()
}

fn key(handle: Handle) -> String {
    format!("{:X}", handle.value())
}

fn msg(m: PcManagerMsg) -> Message {
    Message::PcManager(m)
}

/// One tree row: indent, fold arrow, icon, label and switch (`None` for a
/// row that has none to give, drawn off and disabled).
fn tree_row<'a>(
    state: &PcManager,
    key: String,
    depth: u16,
    fold: Option<bool>,
    icon: Element<'a, Message>,
    label: String,
    switch: Option<(Switch, Message)>,
) -> Element<'a, Message> {
    let enabled = switch.is_some();
    let switch: Element<'a, Message> = match switch {
        // Mixed: a dash in the box, as the reference's indeterminate state.
        Some((Switch::Mixed, message)) => button(
            container(crate::ui::icons::tinted(
                crate::ui::icons::MINUS,
                km::BAR_ICON,
                chrome::accent,
            ))
            .center(Fill),
        )
        .on_press(message)
        .style(kit::flat_button_style(true))
        .padding(0)
        .width(Length::Fixed(km::CHECK))
        .height(Length::Fixed(km::CHECK))
        .into(),
        Some((on, message)) => checkbox(on == Switch::On)
            .on_toggle(move |_| message.clone())
            .size(km::CHECK)
            .into(),
        None => checkbox(false).size(km::CHECK).into(),
    };
    let selected = state.selected.as_deref() == Some(key.as_str());
    let mut row = kit::tree_row(label)
        .depth(depth)
        .icon(icon)
        .trailing(switch)
        .muted(!enabled)
        .selected(selected)
        .on_press(msg(PcManagerMsg::Select(key.clone())));
    row = match fold {
        Some(open) => row.fold(open, msg(PcManagerMsg::Expand(key))),
        None => row.caret_room(),
    };
    row.view()
}

pub fn view<'a>(
    state: &'a PcManager,
    document: &'a Document,
    selected: &[Handle],
    width: f32,
    chrome: crate::ui::dock::Chrome,
) -> Element<'a, Message> {
    let title_bar = crate::ui::dock::title_bar(
        PanelId::PointCloudManager,
        crate::t!("Point Cloud Manager").into_owned(),
        chrome,
    );
    let query = state.search.trim().to_lowercase();
    let matches = |label: &str| query.is_empty() || label.to_lowercase().contains(&query);
    // Searching opens every node so the matches show.
    let open = |key: &str| !query.is_empty() || !state.collapsed.contains(key);
    let regions = crate::t!("Regions").into_owned();
    let unassigned = crate::t!("Unassigned Points").into_owned();
    let scans_label = crate::t!("Scans").into_owned();

    let mut tree = column![];
    for cloud in clouds(document, selected) {
        let root = key(cloud.handle);
        let scans: Vec<&(String, String)> = cloud
            .scans
            .iter()
            .filter(|(name, _)| matches(name))
            .collect();
        let show_scans = matches(&scans_label) || !scans.is_empty();
        let children = matches(&regions) || matches(&unassigned) || show_scans;
        if !matches(&cloud.name) && !children {
            continue;
        }
        let switch = |row: Row| {
            let on = switch_state(&cloud.hidden, &cloud.scans, &row);
            Some((on, msg(PcManagerMsg::Toggle(cloud.handle, row))))
        };
        let cloud_icon = || crate::ui::icons::semantic(CLOUD_ICON, km::ROW_ICON);
        tree = tree.push(tree_row(
            state,
            root.clone(),
            0,
            Some(open(&root)),
            cloud_icon(),
            cloud.name.clone(),
            switch(Row::Cloud),
        ));
        if !open(&root) {
            continue;
        }
        if matches(&regions) {
            // Region data is not read: the row is there, off and disabled.
            tree = tree.push(tree_row(
                state,
                format!("{root}/regions"),
                1,
                None,
                crate::ui::icons::semantic_disabled(REGION_ICON, km::ROW_ICON),
                regions.clone(),
                None,
            ));
        }
        if matches(&unassigned) {
            tree = tree.push(tree_row(
                state,
                format!("{root}/unassigned"),
                1,
                None,
                crate::ui::icons::semantic(UNASSIGNED_ICON, km::ROW_ICON),
                unassigned.clone(),
                switch(Row::Unassigned),
            ));
        }
        if show_scans {
            let scans_key = format!("{root}/scans");
            let scans_open = open(&scans_key);
            tree = tree.push(tree_row(
                state,
                scans_key.clone(),
                1,
                Some(scans_open),
                cloud_icon(),
                scans_label.clone(),
                switch(Row::Scans),
            ));
            if scans_open {
                for (name, id) in scans {
                    tree = tree.push(tree_row(
                        state,
                        format!("{scans_key}/{id}"),
                        2,
                        None,
                        cloud_icon(),
                        name.clone(),
                        switch(Row::Scan(id.clone())),
                    ));
                }
            }
        }
    }

    let header = kit::section_bar(
        crate::t!("REGIONS AND SCANS").into_owned(),
        vec![
            kit::bar_button(
                crate::ui::icons::MINUS,
                crate::t!("Collapse all").into_owned(),
                msg(PcManagerMsg::CollapseAll),
            ),
            kit::bar_button(
                crate::ui::icons::PLUS,
                crate::t!("Expand all").into_owned(),
                msg(PcManagerMsg::ExpandAll),
            ),
        ],
    );
    let search = kit::band(kit::search_field(
        &crate::t!("Search"),
        &state.search,
        |v| msg(PcManagerMsg::Search(v)),
    ));
    kit::palette(
        title_bar,
        column![search, kit::scroll_area(column![header, tree]),],
        width,
    )
}
