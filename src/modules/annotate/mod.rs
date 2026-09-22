// Annotate module — dimension, text, leader, table, and markup tools.

pub mod aligned_dim;
pub mod annotation_scale;
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
        use crate::modules::draw::draw::{centerline, dimcenter, revcloud, wipeout};

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
                            items: crate::modules::ribbon_command_items(&[
                                "MTEXT", "TEXT", "DDEDIT",
                            ]),
                            default: "MTEXT",
                        },
                        RibbonItem::StyleComboGroup {
                            style_key: StyleKey::TextStyle,
                            combo_id: "TEXT_STYLE_COMBO",
                            manager_cmd: Some("STYLE"),
                            rows: vec![vec![crate::modules::ribbon_command("FIND")]],
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
                            items: crate::modules::ribbon_command_items(&[
                                "DIMLINEAR",
                                "DIMALIGNED",
                                "DIMANGULAR",
                                "DIMARC",
                                "DIMRADIUS",
                                "DIMJOGGED",
                                "DIMDIAMETER",
                                "DIMORDINATE",
                                "QDIM",
                            ]),
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
                // ── Centerlines ───────────────────────────────────────────────
                RibbonGroup {
                    title: "Centerlines",
                    tools: vec![
                        RibbonItem::LargeTool(dimcenter::tool()),
                        RibbonItem::LargeTool(centerline::tool()),
                        RibbonItem::Dropdown {
                            id: "ANNOTATE_CENTER_ASSOCIATIVITY",
                            icon: crate::modules::IconKind::Svg(include_bytes!(
                                "../../../assets/icons/line.svg"
                            )),
                            items: crate::modules::ribbon_command_items(&[
                                    "CENTERREASSOCIATE",
                                    "CENTERDISASSOCIATE",
                                    "CENTERRESET",
                            ]),
                            default: "CENTERREASSOCIATE",
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
                            items: crate::modules::ribbon_command_items(&["MLEADER", "LEADER"]),
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
                        RibbonItem::LabeledDropdown {
                            id: "ANNOTATION_CURRENT_SCALE",
                            label: "Add Current Scale",
                            icon: crate::modules::IconKind::Svg(include_bytes!(
                                "../../../assets/icons/add_scale.svg"
                            )),
                            items: crate::modules::ribbon_command_items(&[
                                    "OBJECTSCALE ADD",
                                    "OBJECTSCALE DELETE",
                            ]),
                            default: "OBJECTSCALE ADD",
                        },
                        RibbonItem::Tool(crate::modules::ribbon_command("SCALELISTEDIT")),
                        RibbonItem::Tool(crate::modules::ribbon_command("OBJECTSCALE")),
                        RibbonItem::Tool(crate::modules::ribbon_command("ANNORESET")),
                    ],
                },
            ]
        })
    }
}
