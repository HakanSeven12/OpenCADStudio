// Draw module — Draw, Modify, Annotation and Layer tools.

mod changelog;
pub mod clipboard;
pub mod defaults;
mod donate;
pub mod draw;
pub mod fence;
pub mod units;
pub mod groups;
pub mod inquiry;
pub mod layers;
pub mod modify;
pub mod properties;
mod report;
pub mod select;

use crate::modules::{CadModule, RibbonGroup, RibbonItem};

pub struct DrawModule;

impl CadModule for DrawModule {
    fn id(&self) -> &'static str {
        "draw"
    }
    fn title(&self) -> &'static str {
        "Draw"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        use crate::modules::annotate::{
            angular_dim, leader_cmd, linear_dim, mleader_cmd, mtext, radius_dim, text,
        };
        use crate::modules::insert::{create_block, insert_block};
        use clipboard::{copy_clip, cut, paste};
        use draw::{arc, circle, ellipse, hatch, line, polyline, shapes};
        use groups::{group, ungroup};
        use layers::{
            layfrz, layiso, laylck, layoff, layon, laythw, layulk, layuniso, make_current,
            match_layer, panel,
        };
        use modify::{
            array, copy, delete, explode, fillet, mirror, offset, rotate, scale, stretch,
            translate, trim,
        };
        use inquiry::{area, dist};
        use properties::match_prop;

        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Draw",
                    tools: vec![
                        RibbonItem::LargeTool(line::tool()),
                        RibbonItem::LargeTool(polyline::tool()),
                        RibbonItem::LargeDropdown {
                            id: circle::DROPDOWN_ID,
                            label: "Circle",
                            icon: circle::ICON,
                            items: crate::modules::ribbon_command_items(&[
                                "CIRCLE",
                                "CIRCLE_CD",
                                "CIRCLE_2P",
                                "CIRCLE_3P",
                                "CIRCLE_TTR",
                                "CIRCLE_TTT",
                            ]),
                            default: "CIRCLE",
                        },
                        RibbonItem::LargeDropdown {
                            id: arc::DROPDOWN_ID,
                            label: "Arc",
                            icon: arc::ICON,
                            items: crate::modules::ribbon_command_items(&[
                                "ARC_3P", "ARC_SCE", "ARC_SCA", "ARC_SCL", "ARC_SEA", "ARC_SED",
                                "ARC_SER", "ARC_CSE", "ARC_CSA", "ARC_CSL", "ARC_CONT",
                            ]),
                            default: "ARC_3P",
                        },
                        RibbonItem::Dropdown {
                            id: shapes::DROPDOWN_ID,
                            icon: shapes::ICON,
                            items: crate::modules::ribbon_command_items(&[
                                "RECT", "RECT_ROT", "RECT_CEN", "POLY", "POLY_C", "POLY_E",
                            ]),
                            default: "RECT",
                        },
                        RibbonItem::Dropdown {
                            id: ellipse::DROPDOWN_ID,
                            icon: ellipse::ICON,
                            items: crate::modules::ribbon_command_items(&[
                                "ELLIPSE",
                                "ELLIPSE_AXIS",
                                "ELLIPSE_ARC",
                            ]),
                            default: "ELLIPSE",
                        },
                        RibbonItem::Dropdown {
                            id: hatch::DROPDOWN_ID,
                            icon: hatch::ICON,
                            items: crate::modules::ribbon_command_items(&[
                                "HATCH", "GRADIENT", "BOUNDARY",
                            ]),
                            default: "HATCH",
                        },
                    ],
                },
                RibbonGroup {
                    title: "Modify",
                    tools: vec![
                        translate::tool().into(),
                        copy::tool().into(),
                        stretch::tool().into(),
                        rotate::tool().into(),
                        mirror::tool().into(),
                        scale::tool().into(),
                        RibbonItem::Dropdown {
                            id: trim::DROPDOWN_ID,
                            icon: trim::ICON,
                            items: crate::modules::ribbon_command_items(&["TRIM", "EXTEND"]),
                            default: "TRIM",
                        },
                        RibbonItem::Dropdown {
                            id: fillet::DROPDOWN_ID,
                            icon: fillet::ICON,
                            items: crate::modules::ribbon_command_items(&["FILLET", "CHAMFER"]),
                            default: "FILLET",
                        },
                        RibbonItem::Dropdown {
                            id: array::DROPDOWN_ID,
                            icon: array::ICON,
                            items: crate::modules::ribbon_command_items(&[
                                "ARRAYRECT",
                                "ARRAYPATH",
                                "ARRAYPOLAR",
                            ]),
                            default: "ARRAYRECT",
                        },
                        delete::tool().into(),
                        explode::tool().into(),
                        offset::tool().into(),
                    ],
                },
                RibbonGroup {
                    title: "Annotation",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "ANNOTATION_TEXT",
                            label: "Text",
                            icon: text::ICON,
                            items: crate::modules::ribbon_command_items(&["TEXT", "MTEXT"]),
                            default: "TEXT",
                        },
                        RibbonItem::LargeDropdown {
                            id: "ANNOTATION_DIMENSIONS",
                            label: "Dimensions",
                            icon: linear_dim::ICON,
                            items: crate::modules::ribbon_command_items(&[
                                "DIMLINEAR",
                                "DIMRADIUS",
                                "DIMANGULAR",
                            ]),
                            default: "DIMLINEAR",
                        },
                        RibbonItem::LargeDropdown {
                            id: "ANNOTATION_LEADER",
                            label: "Leader",
                            icon: leader_cmd::ICON,
                            items: crate::modules::ribbon_command_items(&["LEADER", "MLEADER"]),
                            default: "LEADER",
                        },
                    ],
                },
                RibbonGroup {
                    title: "Layers",
                    tools: vec![
                        RibbonItem::LargeTool(panel::tool()),
                        RibbonItem::LayerComboGroup {
                            row2: vec![
                                layoff::tool(),
                                layfrz::tool(),
                                laylck::tool(),
                                make_current::tool(),
                                layiso::tool(),
                            ],
                            row3: vec![
                                layon::tool(),
                                laythw::tool(),
                                layulk::tool(),
                                match_layer::tool(),
                                layuniso::tool(),
                            ],
                        },
                    ],
                },
                RibbonGroup {
                    title: "Block",
                    tools: vec![
                        RibbonItem::LargeTool(create_block::tool()),
                        RibbonItem::LargeTool(insert_block::tool()),
                    ],
                },
                RibbonGroup {
                    title: "Properties",
                    tools: vec![RibbonItem::PropertiesGroup {
                        match_prop: match_prop::tool(),
                    }],
                },
                RibbonGroup {
                    title: "Groups",
                    tools: vec![
                        RibbonItem::LargeTool(group::tool()),
                        RibbonItem::LargeTool(ungroup::tool()),
                    ],
                },
                RibbonGroup {
                    title: "Clipboard",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "PASTE_MENU",
                            label: "Paste",
                            icon: paste::ICON,
                            items: crate::modules::ribbon_command_items(&[
                                "PASTECLIP",
                                "PASTEORIG",
                                "PASTEBLOCK",
                            ]),
                            default: "PASTECLIP",
                        },
                        copy_clip::tool().into(),
                        cut::tool().into(),
                    ],
                },
                RibbonGroup {
                    title: "Measure",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "MEASURE_MENU",
                            label: "Measure",
                            icon: dist::ICON,
                            items: crate::modules::ribbon_command_items(&["DIST", "AREA"]),
                            default: "DIST",
                        },
                    ],
                },
                // Support group lives on the Start tab now (see view.rs:
                // start_page_view). Removed from the Draw ribbon to declutter.
            ]
        })
    }
}
