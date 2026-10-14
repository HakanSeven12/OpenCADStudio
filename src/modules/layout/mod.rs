// Layout module — paper space tools (viewports, scale, plot settings).
// This tab is only shown when the active layout is not "Model".

pub mod mview;
pub mod mvsetup;
pub mod vplayer;

use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, ToolDef};

/// Paper-space context tools, as a flat list for the right-edge side toolbar
/// (the contextual ribbon tab is no longer shown). Viewport + plot actions.
pub fn paper_space_tools() -> Vec<ToolDef> {
    vec![
        mview::tool(),
        ToolDef {
            id: "PAGESETUP",
            label: "Page Setup",
            icon: IconKind::Svg(include_bytes!("../../../assets/icons/pagesetup.svg")),
            event: ModuleEvent::Command("PAGESETUP".to_string()),
        },
        ToolDef {
            id: "PRINTALL",
            label: "Print All",
            icon: IconKind::Svg(include_bytes!("../../../assets/icons/plot.svg")),
            event: ModuleEvent::Command("PRINTALL".to_string()),
        },
    ]
}

pub struct LayoutModule;

impl CadModule for LayoutModule {
    fn id(&self) -> &'static str {
        "layout"
    }
    fn title(&self) -> &'static str {
        "Layout"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Viewport",
                    tools: vec![mview::tool().into()],
                },
                RibbonGroup {
                    title: "Plot",
                    tools: vec![
                        ToolDef {
                            id: "PAGESETUP",
                            label: "Page Setup",
                            icon: IconKind::Svg(include_bytes!(
                                "../../../assets/icons/pagesetup.svg"
                            )),
                            event: ModuleEvent::Command("PAGESETUP".to_string()),
                        }
                        .into(),
                        ToolDef {
                            id: "PRINTALL",
                            label: "Print All",
                            icon: IconKind::Svg(include_bytes!("../../../assets/icons/plot.svg")),
                            event: ModuleEvent::Command("PRINTALL".to_string()),
                        }
                        .into(),
                    ],
                },
            ]
        })
    }
}
