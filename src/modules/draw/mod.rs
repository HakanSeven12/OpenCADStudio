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
                            items: circle::DROPDOWN_ITEMS.to_vec(),
                            default: "CIRCLE",
                        },
                        RibbonItem::LargeDropdown {
                            id: arc::DROPDOWN_ID,
                            label: "Arc",
                            icon: arc::ICON,
                            items: arc::DROPDOWN_ITEMS.to_vec(),
                            default: "ARC_3P",
                        },
                        RibbonItem::Dropdown {
                            id: shapes::DROPDOWN_ID,
                            icon: shapes::ICON,
                            items: shapes::DROPDOWN_ITEMS.to_vec(),
                            default: "RECT",
                        },
                        RibbonItem::Dropdown {
                            id: ellipse::DROPDOWN_ID,
                            icon: ellipse::ICON,
                            items: ellipse::DROPDOWN_ITEMS.to_vec(),
                            default: "ELLIPSE",
                        },
                        RibbonItem::Dropdown {
                            id: hatch::DROPDOWN_ID,
                            icon: hatch::ICON,
                            items: hatch::DROPDOWN_ITEMS.to_vec(),
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
                            items: trim::DROPDOWN_ITEMS.to_vec(),
                            default: "TRIM",
                        },
                        RibbonItem::Dropdown {
                            id: fillet::DROPDOWN_ID,
                            icon: fillet::ICON,
                            items: fillet::DROPDOWN_ITEMS.to_vec(),
                            default: "FILLET",
                        },
                        RibbonItem::Dropdown {
                            id: array::DROPDOWN_ID,
                            icon: array::ICON,
                            items: array::DROPDOWN_ITEMS.to_vec(),
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
                            items: vec![
                                (text::tool().id, text::tool().label, text::tool().icon),
                                (mtext::tool().id, mtext::tool().label, mtext::tool().icon),
                            ],
                            default: "TEXT",
                        },
                        RibbonItem::LargeDropdown {
                            id: "ANNOTATION_DIMENSIONS",
                            label: "Dimensions",
                            icon: linear_dim::ICON,
                            items: vec![
                                (
                                    linear_dim::tool().id,
                                    linear_dim::tool().label,
                                    linear_dim::tool().icon,
                                ),
                                (
                                    radius_dim::tool().id,
                                    radius_dim::tool().label,
                                    radius_dim::tool().icon,
                                ),
                                (
                                    angular_dim::tool().id,
                                    angular_dim::tool().label,
                                    angular_dim::tool().icon,
                                ),
                            ],
                            default: "DIMLINEAR",
                        },
                        RibbonItem::LargeDropdown {
                            id: "ANNOTATION_LEADER",
                            label: "Leader",
                            icon: leader_cmd::ICON,
                            items: vec![
                                (
                                    leader_cmd::tool().id,
                                    leader_cmd::tool().label,
                                    leader_cmd::tool().icon,
                                ),
                                (
                                    mleader_cmd::tool().id,
                                    mleader_cmd::tool().label,
                                    mleader_cmd::tool().icon,
                                ),
                            ],
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
                            items: paste::MENU_ITEMS.to_vec(),
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
                            items: vec![
                                ("DIST", "Distance", dist::ICON),
                                ("AREA", "Area", area::ICON),
                            ],
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
