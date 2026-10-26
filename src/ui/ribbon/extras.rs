//! Host-side extras of ribbon panels: the "more tools" slide-out under a
//! panel's title and the panel's dialog launcher. Plugins describe panels with
//! the plain `RibbonGroup`; everything here is keyed by the panel key the host
//! derives from it, so the plugin API stays unchanged.

use std::sync::OnceLock;

use crate::modules::{IconKind, ModuleEvent, ToolDef};

/// The stable key of the panel `title` on the tab of module `module_id`.
pub(crate) fn panel_key(module_id: &str, title: &str) -> String {
    format!("{module_id}/{title}")
}

/// How a slide-out shows its tools.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Layout {
    /// Rows of icon buttons.
    Grid,
    /// The icon buttons, then the xref fading switch and amount.
    Reference,
}

/// A slide-out tool, with the submenu its arrow opens (empty for a plain
/// button).
pub(crate) struct MoreTool {
    pub tool: ToolDef,
    pub options: &'static [(&'static str, &'static str)],
}

impl MoreTool {
    pub fn command(&self) -> &'static str {
        self.tool.id
    }

    /// Whether the slide-out lights this tool: it or one of its submenu
    /// options runs.
    pub fn is_active(&self, active_tool: Option<&str>) -> bool {
        active_tool.is_some_and(|active| {
            self.command() == active || self.options.iter().any(|(cmd, _)| *cmd == active)
        })
    }
}

/// A titled run of slide-out tools.
pub(crate) struct MoreGroup {
    /// Labels the group once a slide-out has more than one.
    pub title: &'static str,
    pub tools: Vec<MoreTool>,
}

/// What a panel offers beyond its own tools.
pub(crate) struct PanelExtras {
    /// The slide-out's tool groups.
    pub more: Vec<MoreGroup>,
    /// The command the panel's dialog launcher runs.
    pub dialog: Option<&'static str>,
    /// What the launcher's tooltip calls the dialog.
    pub(super) dialog_label: &'static str,
    /// The panel key.
    pub(super) key: &'static str,
    /// The slide-out's dropdown id, which key tips and anchors are keyed on.
    pub(super) id: &'static str,
    pub(super) layout: Layout,
}

impl PanelExtras {
    /// The title of the panel.
    pub fn title(&self) -> &'static str {
        self.key
            .split_once('/')
            .map_or(self.key, |(_, title)| title)
    }

    /// Every slide-out tool, in display order.
    pub fn tools(&self) -> impl Iterator<Item = &MoreTool> {
        self.more.iter().flat_map(|group| &group.tools)
    }
}

pub(super) const DRAW_ID: &str = "draw_extension";
pub(super) const MODIFY_ID: &str = "modify_extension";
pub(super) const REFERENCE_ID: &str = "reference_extension";

fn split(
    command: &'static str,
    label: &'static str,
    icon: &'static [u8],
    options: &'static [(&'static str, &'static str)],
) -> MoreTool {
    MoreTool {
        tool: ToolDef {
            id: command,
            label,
            icon: IconKind::Svg(icon),
            event: ModuleEvent::Command(command.to_string()),
        },
        options,
    }
}

fn tool(command: &'static str, label: &'static str, icon: &'static [u8]) -> MoreTool {
    split(command, label, icon, &[])
}

/// One slide-out with a single tool group named after the panel.
fn extras(
    title: &'static str,
    id: &'static str,
    layout: Layout,
    dialog: Option<(&'static str, &'static str)>,
    tools: Vec<MoreTool>,
) -> PanelExtras {
    PanelExtras {
        more: vec![MoreGroup { title, tools }],
        dialog: dialog.map(|(command, _)| command),
        dialog_label: dialog.map_or("", |(_, label)| label),
        key: "",
        id,
        layout,
    }
}

fn draw_tools() -> Vec<MoreTool> {
    vec![
        tool(
            "SPLINE",
            "Spline Fit",
            include_bytes!("../../../assets/icons/spline.svg"),
        ),
        tool(
            "SPLINECV",
            "Spline CV",
            include_bytes!("../../../assets/icons/spline_cv.svg"),
        ),
        tool(
            "XLINE",
            "Construction Line",
            include_bytes!("../../../assets/icons/xline.svg"),
        ),
        tool(
            "RAY",
            "Ray",
            include_bytes!("../../../assets/icons/ray.svg"),
        ),
        tool(
            "DIVIDE",
            "Divide",
            include_bytes!("../../../assets/icons/divide.svg"),
        ),
        tool(
            "MEASURE",
            "Measure",
            include_bytes!("../../../assets/icons/measure.svg"),
        ),
        tool(
            "REGION",
            "Region",
            include_bytes!("../../../assets/icons/region.svg"),
        ),
        tool(
            "BOUNDARY",
            "Boundary",
            include_bytes!("../../../assets/icons/boundary.svg"),
        ),
        tool(
            "HELIX",
            "Helix",
            include_bytes!("../../../assets/icons/helix.svg"),
        ),
        tool(
            "DONUT",
            "Donut",
            include_bytes!("../../../assets/icons/donut.svg"),
        ),
        split(
            "MULTIPOINT",
            "Multiple Points",
            include_bytes!("../../../assets/icons/multipoint.svg"),
            &[
                ("POINT", "Single Point"),
                ("MULTIPOINT", "Multiple Points"),
                ("DDPTYPE", "Point Style"),
            ],
        ),
    ]
}

fn modify_tools() -> Vec<MoreTool> {
    vec![
        tool(
            "SETBYLAYER",
            "Set to ByLayer",
            include_bytes!("../../../assets/icons/modify_setbylayer.svg"),
        ),
        tool(
            "LENGTHEN",
            "Lengthen",
            include_bytes!("../../../assets/icons/modify_lengthen.svg"),
        ),
        tool(
            "PEDIT",
            "Edit Polyline",
            include_bytes!("../../../assets/icons/modify_pedit.svg"),
        ),
        tool(
            "SPLINEDIT",
            "Edit Spline",
            include_bytes!("../../../assets/icons/modify_splinedit.svg"),
        ),
        tool(
            "HATCHEDIT",
            "Edit Hatch",
            include_bytes!("../../../assets/icons/modify_hatchedit.svg"),
        ),
        tool(
            "ALIGN",
            "Align",
            include_bytes!("../../../assets/icons/modify_align.svg"),
        ),
        // ALIGNLEFT/ALIGNHCENTER/ALIGNRIGHT/ALIGNTOP/ALIGNVCENTER/ALIGNBOTTOM:
        // one-shot bounding-box alignment of the current selection — distinct
        // from "ALIGN" above (point-picked 3D placement). No point prompts, so
        // each is a plain one-click tool rather than a dropdown submenu,
        // matching this panel's existing flat-list convention.
        tool(
            "ALIGNLEFT",
            "Align Left",
            include_bytes!("../../../assets/icons/align_left.svg"),
        ),
        tool(
            "ALIGNHCENTER",
            "Align Horizontal Centers",
            include_bytes!("../../../assets/icons/align_hcenter.svg"),
        ),
        tool(
            "ALIGNRIGHT",
            "Align Right",
            include_bytes!("../../../assets/icons/align_right.svg"),
        ),
        tool(
            "ALIGNTOP",
            "Align Top",
            include_bytes!("../../../assets/icons/align_top.svg"),
        ),
        tool(
            "ALIGNVCENTER",
            "Align Vertical Centers",
            include_bytes!("../../../assets/icons/align_vcenter.svg"),
        ),
        tool(
            "ALIGNBOTTOM",
            "Align Bottom",
            include_bytes!("../../../assets/icons/align_bottom.svg"),
        ),
        tool(
            "BREAK",
            "Break",
            include_bytes!("../../../assets/icons/modify_break.svg"),
        ),
        tool(
            "BREAKATPOINT",
            "Break at Point",
            include_bytes!("../../../assets/icons/modify_breakatpoint.svg"),
        ),
        tool(
            "JOIN",
            "Join",
            include_bytes!("../../../assets/icons/modify_join.svg"),
        ),
        tool(
            "REVERSE",
            "Reverse",
            include_bytes!("../../../assets/icons/modify_reverse.svg"),
        ),
        tool(
            "NCOPY",
            "Copy Nested Objects",
            include_bytes!("../../../assets/icons/modify_ncopy.svg"),
        ),
        split(
            "DRAWORDER_FRONT",
            "Draw Order",
            include_bytes!("../../../assets/icons/modify_draworder.svg"),
            &[
                ("DRAWORDER_FRONT", "Bring to Front"),
                ("DRAWORDER_BACK", "Send to Back"),
                ("DRAWORDER_ABOVE", "Bring Above Objects"),
                ("DRAWORDER_UNDER", "Send Under Objects"),
            ],
        ),
    ]
}

fn build() -> Vec<(&'static str, PanelExtras)> {
    let attdef = || {
        tool(
            "ATTDEF",
            "Define Attributes",
            include_bytes!("../../../assets/icons/attdef.svg"),
        )
    };
    let attman = || {
        tool(
            "ATTMAN",
            "Manage Attributes",
            include_bytes!("../../../assets/icons/attman.svg"),
        )
    };
    let attsync = || {
        tool(
            "ATTSYNC",
            "Synchronize",
            include_bytes!("../../../assets/icons/attsync.svg"),
        )
    };
    let base = || {
        tool(
            "BASE",
            "Set Base Point",
            include_bytes!("../../../assets/icons/base_point.svg"),
        )
    };
    vec![
        (
            "draw/Draw",
            extras("Draw", DRAW_ID, Layout::Grid, None, draw_tools()),
        ),
        (
            "draw/Modify",
            extras("Modify", MODIFY_ID, Layout::Grid, None, modify_tools()),
        ),
        (
            "draw/Block",
            extras(
                "Block",
                "home_block_extension",
                Layout::Grid,
                None,
                vec![attdef(), attman(), attsync(), base()],
            ),
        ),
        (
            "insert/Block Definition",
            extras(
                "Block Definition",
                "block_definition_extension",
                Layout::Grid,
                None,
                vec![base(), attsync()],
            ),
        ),
        // Edit Reference, then the xref fading switch and amount; the corner
        // launcher opens the External References palette.
        (
            "insert/Reference",
            extras(
                "Reference",
                REFERENCE_ID,
                Layout::Reference,
                Some(("EXTERNALREFERENCES", "External References")),
                vec![tool(
                    "REFEDIT",
                    "Edit Reference",
                    include_bytes!("../../../assets/icons/edit_block.svg"),
                )],
            ),
        ),
        (
            "insert/Import",
            extras(
                "Import",
                "import_extension",
                Layout::Grid,
                None,
                vec![tool(
                    "LANDXMLIMPORT",
                    "Land XML",
                    include_bytes!("../../../assets/icons/landxml.svg"),
                )],
            ),
        ),
    ]
}

fn registry() -> &'static [(&'static str, PanelExtras)] {
    static REGISTRY: OnceLock<Vec<(&'static str, PanelExtras)>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        build()
            .into_iter()
            .map(|(key, mut extras)| {
                extras.key = key;
                (key, extras)
            })
            .collect()
    })
}

/// The extras of the panel `key`, if it has any.
pub(crate) fn for_panel(key: &str) -> Option<&'static PanelExtras> {
    registry()
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, extras)| extras)
}

/// Every panel's extras.
pub(super) fn all() -> impl Iterator<Item = &'static PanelExtras> {
    registry().iter().map(|(_, extras)| extras)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_key_joins_module_id_and_title() {
        assert_eq!(
            panel_key("insert", "Block Definition"),
            "insert/Block Definition"
        );
    }

    #[test]
    fn every_key_names_a_core_panel() {
        let modules = crate::modules::registry::all_modules();
        let keys: Vec<String> = modules
            .iter()
            .flat_map(|module| {
                module
                    .ribbon_groups()
                    .iter()
                    .map(|group| panel_key(module.id(), group.title))
            })
            .collect();
        for (key, _) in registry() {
            assert!(keys.iter().any(|k| k == key), "{key} is not a ribbon panel");
        }
    }

    #[test]
    fn every_entry_knows_its_key_and_title() {
        let draw = for_panel("draw/Draw").unwrap();
        assert_eq!(draw.key, "draw/Draw");
        assert_eq!(draw.title(), "Draw");
        assert_eq!(
            for_panel("insert/Block Definition").unwrap().title(),
            "Block Definition"
        );
    }

    #[test]
    fn home_block_has_a_slide_out_and_insert_block_does_not() {
        assert!(for_panel("draw/Block").is_some());
        assert!(for_panel("insert/Block").is_none());
    }

    #[test]
    fn slide_out_ids_are_unique() {
        let ids: Vec<_> = all().map(|e| e.id).collect();
        for (i, id) in ids.iter().enumerate() {
            assert!(!ids[..i].contains(id), "{id} is used twice");
        }
    }

    #[test]
    fn every_tool_command_is_registered() {
        let registered = crate::command::all_registered_command_names();
        for command in all().flat_map(|extras| extras.tools()).flat_map(|tool| {
            std::iter::once(tool.command()).chain(tool.options.iter().map(|o| o.0))
        }) {
            assert!(registered.contains(&command), "{command} is not registered");
        }
    }

    #[test]
    fn split_tools_follow_plain_tools() {
        for extras in all() {
            let tools: Vec<_> = extras.tools().collect();
            let first_split = tools
                .iter()
                .position(|tool| !tool.options.is_empty())
                .unwrap_or(tools.len());
            assert!(tools[first_split..]
                .iter()
                .all(|tool| !tool.options.is_empty()));
        }
    }
}
