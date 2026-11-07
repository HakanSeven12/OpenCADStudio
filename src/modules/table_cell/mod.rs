//! Contextual "Table Cell" ribbon tab, shown only while table cells are
//! picked: rows, columns, merging, cell styles and formats, inserts and
//! linked data. The same tools sit on the right-edge cell palette.

use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct TableCellModule;

impl CadModule for TableCellModule {
    fn id(&self) -> &'static str {
        "table_cell"
    }

    fn title(&self) -> &'static str {
        "Table Cell"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            let large = |id: &'static str, label: &'static str, icon: &'static [u8]| {
                RibbonItem::LargeTool(ToolDef {
                    id,
                    label,
                    icon: IconKind::Svg(icon),
                    event: ModuleEvent::Command(id.to_string()),
                })
            };
            let small = |id: &'static str, label: &'static str, icon: &'static [u8]| {
                RibbonItem::LabeledTool(ToolDef {
                    id,
                    label,
                    icon: IconKind::Svg(icon),
                    event: ModuleEvent::Command(id.to_string()),
                })
            };
            let alignments = crate::ui::ribbon::table_cell_alignments()
                .into_iter()
                .map(|t| (t.id, t.label, t.icon))
                .collect();
            vec![
                RibbonGroup {
                    title: "Rows",
                    tools: vec![
                        large("_TCELLROWABOVE", "Insert\nAbove", include_bytes!("../../../assets/icons/table_row_above.svg")),
                        large("_TCELLROWBELOW", "Insert\nBelow", include_bytes!("../../../assets/icons/table_row_below.svg")),
                        large("_TCELLDELROWS", "Delete\nRow(s)", include_bytes!("../../../assets/icons/table_row_delete.svg")),
                    ],
                },
                RibbonGroup {
                    title: "Columns",
                    tools: vec![
                        large("_TCELLCOLLEFT", "Insert\nLeft", include_bytes!("../../../assets/icons/table_col_left.svg")),
                        large("_TCELLCOLRIGHT", "Insert\nRight", include_bytes!("../../../assets/icons/table_col_right.svg")),
                        large("_TCELLDELCOLS", "Delete\nColumn(s)", include_bytes!("../../../assets/icons/table_col_delete.svg")),
                    ],
                },
                RibbonGroup {
                    title: "Merge",
                    tools: vec![
                        large("_TCELLMERGE", "Merge\nCells", include_bytes!("../../../assets/icons/table_merge.svg")),
                        large("_TCELLUNMERGE", "Unmerge\nCells", include_bytes!("../../../assets/icons/table_unmerge.svg")),
                    ],
                },
                RibbonGroup {
                    title: "Cell Styles",
                    tools: vec![
                        large("_TCELLMATCH", "Match\nCell", include_bytes!("../../../assets/icons/match_prop.svg")),
                        RibbonItem::LargeDropdown {
                            id: "TCELL_ALIGNMENT",
                            label: "Alignment",
                            icon: IconKind::Svg(include_bytes!("../../../assets/icons/table_align_mc.svg")),
                            items: alignments,
                            default: "_TCELLALIGN MC",
                        },
                        large("_TCELLBORDERS", "Cell\nBorders", include_bytes!("../../../assets/icons/table_cell_borders.svg")),
                    ],
                },
                RibbonGroup {
                    title: "Cell Format",
                    tools: vec![
                        small("_TCELLLOCK", "Cell Locking", include_bytes!("../../../assets/icons/table_cell_lock.svg")),
                        small("_TCELLFORMAT", "Data Format", include_bytes!("../../../assets/icons/table_cell_format.svg")),
                    ],
                },
                RibbonGroup {
                    title: "Insert",
                    tools: vec![
                        large("_TCELLBLOCK", "Block", include_bytes!("../../../assets/icons/edit_block.svg")),
                        large("_TCELLFIELD", "Field", include_bytes!("../../../assets/icons/text.svg")),
                        large("_TCELLFORMULA", "Formula", include_bytes!("../../../assets/icons/table_cell_formula.svg")),
                        large("_TCELLCONTENTS", "Manage Cell\nContents", include_bytes!("../../../assets/icons/table_cell_contents.svg")),
                    ],
                },
                RibbonGroup {
                    title: "Data",
                    tools: vec![
                        large("_TCELLLINK", "Link\nCell", include_bytes!("../../../assets/icons/data_link.svg")),
                        large("_TCELLDOWNLOAD", "Download\nfrom Source", include_bytes!("../../../assets/icons/sync.svg")),
                    ],
                },
            ]
        })
    }
}
