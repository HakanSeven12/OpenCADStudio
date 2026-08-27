// Annotate module — dimension, text, leader, table, and markup tools.

pub mod aligned_dim;
pub mod arc_length_dim;
pub mod angular_dim;
pub mod data_extract;
pub mod data_link;
pub mod ddedit;
pub mod diameter_dim;
pub mod dim_baseline;
pub mod dim_continue;
pub mod dimbreak;
pub mod dimedit;
pub mod dimjogline;
pub mod dimspace;
pub mod dimtedit;
pub mod leader_cmd;
pub mod linear_dim;
pub mod jogged_radius_dim;
pub mod mleader_cmd;
pub mod mleader_edit;
pub mod mtext;
pub mod ordinate_dim;
pub mod qdim;
pub mod radius_dim;
pub mod table_cmd;
pub mod text;
pub mod textedit;
pub mod tolerance_cmd;

use crate::modules::{CadModule, RibbonGroup, RibbonItem, StyleKey};

pub struct AnnotateModule;

impl CadModule for AnnotateModule {
    fn id(&self) -> &'static str {
        "annotate"
    }
    fn title(&self) -> &'static str {
        "Annotate"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        use crate::modules::draw::draw::{revcloud, wipeout};

        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                // ── Text ─────────────────────────────────────────────────────
                RibbonGroup {
                    title: "Text",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "ANNOTATE_TEXT",
                            label: "Multiline\nText",
                            icon: mtext::ICON,
                            items: vec![
                                (mtext::tool().id, mtext::tool().label, mtext::tool().icon),
                                (text::tool().id, text::tool().label, text::tool().icon),
                                (ddedit::tool().id, ddedit::tool().label, ddedit::tool().icon),
                            ],
                            default: "MTEXT",
                        },
                        RibbonItem::StyleComboGroup {
                            style_key: StyleKey::TextStyle,
                            combo_id: "TEXT_STYLE_COMBO",
                            manager_cmd: Some("STYLE"),
                            rows: vec![vec![crate::modules::ToolDef {
                                id: "FIND",
                                label: "Find",
                                icon: crate::modules::IconKind::Svg(include_bytes!(
                                    "../../../assets/icons/find.svg"
                                )),
                                event: crate::modules::ModuleEvent::Command("FIND".to_string()),
                            }]],
                        },
                    ],
                },
                // ── Dimensions ───────────────────────────────────────────────
                RibbonGroup {
                    title: "Dimensions",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "ANNOTATE_DIM",
                            label: "Dimension",
                            icon: linear_dim::ICON,
                            items: vec![
                                (
                                    linear_dim::tool().id,
                                    linear_dim::tool().label,
                                    linear_dim::tool().icon,
                                ),
                                (
                                    aligned_dim::tool().id,
                                    aligned_dim::tool().label,
                                    aligned_dim::tool().icon,
                                ),
                                (
                                    angular_dim::tool().id,
                                    angular_dim::tool().label,
                                    angular_dim::tool().icon,
                                ),
                                (
                                    arc_length_dim::tool().id,
                                    arc_length_dim::tool().label,
                                    arc_length_dim::tool().icon,
                                ),
                                (
                                    radius_dim::tool().id,
                                    radius_dim::tool().label,
                                    radius_dim::tool().icon,
                                ),
                                (
                                    jogged_radius_dim::tool().id,
                                    jogged_radius_dim::tool().label,
                                    jogged_radius_dim::tool().icon,
                                ),
                                (
                                    diameter_dim::tool().id,
                                    diameter_dim::tool().label,
                                    diameter_dim::tool().icon,
                                ),
                                (
                                    ordinate_dim::tool().id,
                                    ordinate_dim::tool().label,
                                    ordinate_dim::tool().icon,
                                ),
                                (qdim::tool().id, qdim::tool().label, qdim::tool().icon),
                            ],
                            default: "DIMLINEAR",
                        },
                        RibbonItem::StyleComboGroup {
                            style_key: StyleKey::DimStyle,
                            combo_id: "DIM_STYLE_COMBO",
                            manager_cmd: Some("DIMSTYLE"),
                            rows: vec![
                                vec![qdim::tool(), dim_continue::tool(), dim_baseline::tool()],
                                vec![
                                    tolerance_cmd::tool(),
                                    dimedit::tool(),
                                    dimtedit::tool(),
                                    dimbreak::tool(),
                                    dimspace::tool(),
                                    dimjogline::tool(),
                                ],
                            ],
                        },
                    ],
                },
                // ── Leaders ──────────────────────────────────────────────────
                RibbonGroup {
                    title: "Leaders",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "ANNOTATE_LEADER",
                            label: "Multileader",
                            icon: mleader_cmd::ICON,
                            items: vec![
                                (
                                    mleader_cmd::tool().id,
                                    mleader_cmd::tool().label,
                                    mleader_cmd::tool().icon,
                                ),
                                (
                                    leader_cmd::tool().id,
                                    leader_cmd::tool().label,
                                    leader_cmd::tool().icon,
                                ),
                            ],
                            default: "MLEADER",
                        },
                        RibbonItem::StyleComboGroup {
                            style_key: StyleKey::MLeaderStyle,
                            combo_id: "MLEADER_STYLE_COMBO",
                            manager_cmd: Some("MLEADERSTYLE"),
                            rows: vec![
                                vec![mleader_edit::tool_add(), mleader_edit::tool_remove()],
                                vec![mleader_edit::tool_align(), mleader_edit::tool_collect()],
                            ],
                        },
                    ],
                },
                // ── Tables ───────────────────────────────────────────────────
                RibbonGroup {
                    title: "Tables",
                    tools: vec![
                        RibbonItem::LargeTool(table_cmd::tool()),
                        RibbonItem::StyleComboGroup {
                            style_key: StyleKey::TableStyle,
                            combo_id: "TABLE_STYLE_COMBO",
                            manager_cmd: Some("TABLESTYLE"),
                            rows: vec![vec![data_extract::tool(), data_link::tool()]],
                        },
                    ],
                },
                // ── Markup ───────────────────────────────────────────────────
                RibbonGroup {
                    title: "Markup",
                    tools: vec![
                        RibbonItem::LargeTool(wipeout::tool()),
                        RibbonItem::LargeTool(revcloud::tool()),
                    ],
                },
                // ── Annotation Scaling ───────────────────────────────────────
                RibbonGroup {
                    title: "Annotation Scaling",
                    tools: vec![
                        RibbonItem::Tool(crate::modules::ToolDef {
                            id: "ANNOSCALE",
                            label: "Scale List",
                            icon: crate::modules::IconKind::Svg(include_bytes!(
                                "../../../assets/icons/scale_list.svg"
                            )),
                            event: crate::modules::ModuleEvent::Command("ANNOSCALE".to_string()),
                        }),
                        RibbonItem::Tool(crate::modules::ToolDef {
                            id: "OBJECTSCALE",
                            label: "Add Scale",
                            icon: crate::modules::IconKind::Svg(include_bytes!(
                                "../../../assets/icons/add_scale.svg"
                            )),
                            // The quick "mark annotative at the current scale"
                            // action; bare OBJECTSCALE opens the manage dialog.
                            event: crate::modules::ModuleEvent::Command(
                                "OBJECTSCALE ADD".to_string(),
                            ),
                        }),
                        RibbonItem::Tool(crate::modules::ToolDef {
                            id: "SCALELISTEDIT",
                            label: "Scale Edit",
                            icon: crate::modules::IconKind::Svg(include_bytes!(
                                "../../../assets/icons/scale_list.svg"
                            )),
                            event: crate::modules::ModuleEvent::Command(
                                "SCALELISTEDIT".to_string(),
                            ),
                        }),
                        RibbonItem::Tool(crate::modules::ToolDef {
                            id: "SYNCPVIEWPORTS",
                            label: "Sync Scales",
                            icon: crate::modules::IconKind::Svg(include_bytes!(
                                "../../../assets/icons/sync.svg"
                            )),
                            event: crate::modules::ModuleEvent::Command(
                                "SYNCPVIEWPORTS".to_string(),
                            ),
                        }),
                    ],
                },
            ]
        })
    }
}
