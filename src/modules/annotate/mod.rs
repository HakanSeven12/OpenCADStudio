// Annotate module — dimension, text, leader, table, and markup tools.

pub mod aligned_dim;
pub mod angular_dim;
pub mod annotation_scale;
pub mod arc_length_dim;
pub mod count_cmd;
pub mod data_extract;
pub mod data_link;
pub mod ddedit;
pub mod diameter_dim;
pub mod dim_baseline;
pub mod dim_cmd;
pub mod dim_continue;
pub mod dim_inspect;
pub mod dimbreak;
pub mod dimedit;
pub mod dimoverride;
pub mod dimjogline;
pub mod dimreassociate;
pub mod dim_distribute;
pub mod dimspace;
pub mod dimtedit;
pub mod field_cmd;
pub mod jogged_radius_dim;
pub mod leader_cmd;
pub mod linear_dim;
pub mod mleader_cmd;
pub mod mleader_edit;
pub mod mtext;
pub mod ordinate_dim;
pub mod qdim;
pub mod radius_dim;
pub mod table_cmd;
pub mod text;
pub mod text_edit;
pub mod textedit;
pub mod txt2mtxt;
pub mod tolerance_cmd;

use crate::modules::{CadModule, IconKind, RibbonGroup, RibbonItem, StyleKey};

/// The dimension tools one ribbon dropdown offers — shared by the Annotate
/// and Draw tabs so both list the same set.
pub fn dimension_menu_items() -> Vec<(&'static str, &'static str, IconKind)> {
    vec![
        (linear_dim::tool().id, linear_dim::tool().label, linear_dim::tool().icon),
        (aligned_dim::tool().id, aligned_dim::tool().label, aligned_dim::tool().icon),
        (angular_dim::tool().id, angular_dim::tool().label, angular_dim::tool().icon),
        (arc_length_dim::tool().id, arc_length_dim::tool().label, arc_length_dim::tool().icon),
        (radius_dim::tool().id, radius_dim::tool().label, radius_dim::tool().icon),
        (
            jogged_radius_dim::tool().id,
            jogged_radius_dim::tool().label,
            jogged_radius_dim::tool().icon,
        ),
        (diameter_dim::tool().id, diameter_dim::tool().label, diameter_dim::tool().icon),
        (ordinate_dim::tool().id, ordinate_dim::tool().label, ordinate_dim::tool().icon),
        (qdim::tool().id, qdim::tool().label, qdim::tool().icon),
    ]
}

/// The Dimensions panel's Linear split button, in the reference order.
fn annotate_dimension_items() -> Vec<(&'static str, &'static str, IconKind)> {
    vec![
        (linear_dim::tool().id, "Linear", linear_dim::tool().icon),
        (aligned_dim::tool().id, "Aligned", aligned_dim::tool().icon),
        (angular_dim::tool().id, "Angular", angular_dim::tool().icon),
        (arc_length_dim::tool().id, "Arc Length", arc_length_dim::tool().icon),
        (radius_dim::tool().id, "Radius", radius_dim::tool().icon),
        (diameter_dim::tool().id, "Diameter", diameter_dim::tool().icon),
        (jogged_radius_dim::tool().id, "Jogged", jogged_radius_dim::tool().icon),
        (ordinate_dim::tool().id, "Ordinate", ordinate_dim::tool().icon),
    ]
}

/// A small Dimensions-panel button running `command`.
fn dim_tool(id: &'static str, label: &'static str, icon: &'static [u8], command: &str) -> crate::modules::ToolDef {
    crate::modules::ToolDef {
        id,
        label,
        icon: IconKind::Svg(icon),
        event: crate::modules::ModuleEvent::Command(command.to_string()),
    }
}

/// A text height the way the Text panel shows it (no trailing zeros).
pub fn text_height_label(height: f64) -> String {
    let text = format!("{height:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn text_tool(id: &'static str, label: &'static str, icon: &'static [u8]) -> crate::modules::ToolDef {
    crate::modules::ToolDef {
        id,
        label,
        icon: IconKind::Svg(icon),
        event: crate::modules::ModuleEvent::Command(id.to_string()),
    }
}

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
                        // The face runs and shows the one chosen last.
                        RibbonItem::LargeDropdown {
                            id: "ANNOTATE_TEXT",
                            label: ocs_plugin_api::ribbon::FOLLOW_CHOSEN,
                            icon: mtext::ICON,
                            items: vec![
                                (mtext::tool().id, "Multiline Text", mtext::tool().icon),
                                (text::tool().id, "Single Line", text::tool().icon),
                            ],
                            default: "MTEXT",
                        },
                        RibbonItem::ToolGrid {
                            columns: vec![vec![
                                text_tool(
                                    "SPELL",
                                    "Check Spelling",
                                    include_bytes!("../../../assets/icons/ui/check.svg"),
                                ),
                                text_tool(
                                    "TEXTALIGN",
                                    "Text Align",
                                    include_bytes!("../../../assets/icons/align_left.svg"),
                                ),
                                text_tool(
                                    "JUSTIFYTEXT",
                                    "Justify",
                                    include_bytes!("../../../assets/icons/mt_align_justify.svg"),
                                ),
                            ]],
                        },
                        // Text style, then the find box and the text height
                        // (drawn by the ribbon for the text style group).
                        RibbonItem::StyleComboGroup {
                            style_key: StyleKey::TextStyle,
                            combo_id: "TEXT_STYLE_COMBO",
                            manager_cmd: Some("STYLE"),
                            rows: Vec::new(),
                        },
                    ],
                },
                // ── Dimensions ───────────────────────────────────────────────
                // Dimension (DIM), the style list with Break / Adjust Space /
                // Jog Line, the dimension layer list with Inspect / Update /
                // Reassociate, then the Linear and Continue split buttons
                // and Quick. Tolerance and the text edits are in the
                // slide-out; the corner arrow opens Dimension Style.
                RibbonGroup {
                    title: "Dimensions",
                    tools: vec![
                        RibbonItem::LargeTool(crate::modules::ToolDef {
                            id: "DIM",
                            label: "Dimension",
                            icon: linear_dim::ICON,
                            event: crate::modules::ModuleEvent::Command("DIM".to_string()),
                        }),
                        RibbonItem::StyleComboGroup {
                            style_key: StyleKey::DimStyle,
                            combo_id: "DIM_STYLE_COMBO",
                            manager_cmd: Some("DIMSTYLE"),
                            rows: vec![
                                vec![
                                    dim_tool("DIMBREAK", "Break", include_bytes!("../../../assets/icons/dim_break.svg"), "DIMBREAK"),
                                    dim_tool("DIMSPACE", "Adjust Space", include_bytes!("../../../assets/icons/dim_space.svg"), "DIMSPACE"),
                                    dim_tool("DIMJOGLINE", "Jog Line", include_bytes!("../../../assets/icons/dim_jog.svg"), "DIMJOGLINE"),
                                ],
                                vec![
                                    dim_tool("DIMINSPECT", "Inspect", include_bytes!("../../../assets/icons/find.svg"), "DIMINSPECT"),
                                    dim_tool("DIMSTYLE_APPLY", "Update", include_bytes!("../../../assets/icons/ui/refresh.svg"), r#"-DIMSTYLE ["APPLY"]"#),
                                    dim_tool("DIMREASSOCIATE", "Reassociate", include_bytes!("../../../assets/icons/sync.svg"), "DIMREASSOCIATE"),
                                ],
                            ],
                        },
                        RibbonItem::LabeledDropdown {
                            id: "ANNOTATE_DIM",
                            label: "",
                            icon: linear_dim::ICON,
                            items: annotate_dimension_items(),
                            default: "DIMLINEAR",
                        },
                        RibbonItem::LabeledTool(crate::modules::ToolDef {
                            id: "QDIM",
                            label: "Quick",
                            icon: qdim::tool().icon,
                            event: crate::modules::ModuleEvent::Command("QDIM".to_string()),
                        }),
                        RibbonItem::LabeledDropdown {
                            id: "ANNOTATE_DIM_CONTINUE",
                            label: "",
                            icon: dim_continue::tool().icon,
                            items: vec![
                                (dim_continue::tool().id, "Continue", dim_continue::tool().icon),
                                (dim_baseline::tool().id, "Baseline", dim_baseline::tool().icon),
                            ],
                            default: "DIMCONTINUE",
                        },
                    ],
                },
                // ── Centerlines ───────────────────────────────────────────────
                // Center Mark and Centerline; the slide-out holds the layer
                // new centre objects go on (CENTERLAYER).
                RibbonGroup {
                    title: "Centerlines",
                    tools: vec![
                        RibbonItem::LargeTool(dimcenter::tool()),
                        RibbonItem::LargeTool(centerline::tool()),
                    ],
                },
                // ── Leaders ──────────────────────────────────────────────────
                // Multileader, the style list, the multileader layer list,
                // then Align / Collect / Add Leader / Remove Leader; the
                // corner arrow opens Multileader Style.
                RibbonGroup {
                    title: "Leaders",
                    tools: vec![
                        RibbonItem::LargeTool(crate::modules::ToolDef {
                            id: "MLEADER",
                            label: "Multileader",
                            icon: mleader_cmd::ICON,
                            event: crate::modules::ModuleEvent::Command("MLEADER".to_string()),
                        }),
                        RibbonItem::StyleComboGroup {
                            style_key: StyleKey::MLeaderStyle,
                            combo_id: "MLEADER_STYLE_COMBO",
                            manager_cmd: Some("MLEADERSTYLE"),
                            rows: vec![vec![
                                mleader_edit::tool_align(),
                                mleader_edit::tool_collect(),
                                mleader_edit::tool_add(),
                                mleader_edit::tool_remove(),
                            ]],
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
                            // Each row leads with an icon-only update button,
                            // then the tool named beside its icon.
                            rows: vec![
                                vec![
                                    data_link::download_tool(),
                                    crate::modules::ToolDef {
                                        label: "Extract Data",
                                        ..data_extract::tool()
                                    },
                                ],
                                vec![
                                    data_link::upload_tool(),
                                    crate::modules::ToolDef {
                                        label: "Link Data",
                                        ..data_link::tool()
                                    },
                                ],
                            ],
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
                            items: vec![
                                (
                                    "OBJECTSCALE ADD",
                                    "Add Current Scale",
                                    crate::modules::IconKind::Svg(include_bytes!(
                                        "../../../assets/icons/add_scale.svg"
                                    )),
                                ),
                                (
                                    "OBJECTSCALE DELETE",
                                    "Delete Current Scale",
                                    crate::modules::IconKind::Svg(include_bytes!(
                                        "../../../assets/icons/add_scale.svg"
                                    )),
                                ),
                            ],
                            default: "OBJECTSCALE ADD",
                        },
                        RibbonItem::Tool(crate::modules::ToolDef {
                            id: "SCALELISTEDIT",
                            label: "Scale List",
                            icon: crate::modules::IconKind::Svg(include_bytes!(
                                "../../../assets/icons/scale_list.svg"
                            )),
                            event: crate::modules::ModuleEvent::Command(
                                "SCALELISTEDIT".to_string(),
                            ),
                        }),
                        RibbonItem::Tool(crate::modules::ToolDef {
                            id: "OBJECTSCALE",
                            label: "Add/Delete Scales",
                            icon: crate::modules::IconKind::Svg(include_bytes!(
                                "../../../assets/icons/add_scale.svg"
                            )),
                            event: crate::modules::ModuleEvent::Command("OBJECTSCALE".to_string()),
                        }),
                        RibbonItem::Tool(crate::modules::ToolDef {
                            id: "ANNORESET",
                            label: "Sync Scale Positions",
                            icon: crate::modules::IconKind::Svg(include_bytes!(
                                "../../../assets/icons/sync.svg"
                            )),
                            event: crate::modules::ModuleEvent::Command("ANNORESET".to_string()),
                        }),
                    ],
                },
            ]
        })
    }
}
