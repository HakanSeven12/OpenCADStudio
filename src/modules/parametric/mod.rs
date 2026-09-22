//! Ribbon tools for persistent geometric and dimensional constraints.
//! Commands collect their input here; the scene stores and re-solves the
//! resulting constraints through the geometry kernel.

mod coincident;
mod concentric;
mod constraint_bar;
mod equal_distance;
#[path = "equal.rs"]
mod equal_command;
#[path = "fixed.rs"]
mod fixed_command;
mod geom_constraint;
#[path = "horizontal.rs"]
mod horizontal_command;
#[path = "perpendicular.rs"]
mod perpendicular_command;
mod point_on_entity;
mod smooth;
#[path = "symmetric.rs"]
mod symmetric_command;
#[path = "tangent.rs"]
mod tangent_command;
mod tools;
mod value;
pub use coincident::{coincident_tool, CoincidentConstraintCommand};
pub use concentric::ConcentricConstraintCommand;
pub use constraint_bar::ConstraintBarOptionCommand;
pub use equal_command::EqualConstraintCommand;
pub use equal_distance::{equal_distance_tool, EqualDistanceConstraintCommand};
pub use fixed_command::FixConstraintCommand;
pub use geom_constraint::GeomConstraintCommand;
pub use horizontal_command::HorizontalConstraintCommand;
pub use perpendicular_command::{PerpendicularConstraintCommand, PerpendicularPick};
pub use point_on_entity::{
    center_point_tool, midpoint_tool, point_on_curve_tool, PointOnEntityConstraintCommand,
};
pub use smooth::SmoothConstraintCommand;
pub use symmetric_command::SymmetricConstraintCommand;
pub use tangent_command::TangentConstraintCommand;
pub use tools::{
    colinear, concentric as concentric_tool, equal, fixed, horizontal, normal, parallel,
    perpendicular, symmetric, tangent, vertical,
};
pub use value::{
    angle_tool, dimensional_tools, distance_tool, AngleConstraintCommand,
    DistanceConstraintCommand, DistanceMode,
};

use crate::modules::{CadModule, IconKind, RibbonGroup, RibbonItem};

pub struct ParametricModule;

impl CadModule for ParametricModule {
    fn id(&self) -> &'static str {
        "parametric"
    }

    fn title(&self) -> &'static str {
        "Parametric"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Geometric",
                    tools: vec![
                        RibbonItem::LargeTool(crate::modules::ribbon_command("AUTOCONSTRAIN")),
                        RibbonItem::LargeTool(coincident_tool::tool()),
                        RibbonItem::LargeTool(parallel::tool()),
                        RibbonItem::LargeTool(tangent::tool()),
                        RibbonItem::LargeTool(colinear::tool()),
                        RibbonItem::LargeTool(perpendicular::tool()),
                        RibbonItem::LargeTool(crate::modules::ribbon_command("GCSMOOTH")),
                        RibbonItem::LargeTool(concentric_tool::tool()),
                        RibbonItem::LargeTool(horizontal::tool()),
                        RibbonItem::LargeTool(symmetric::tool()),
                        RibbonItem::LargeTool(fixed::tool()),
                        RibbonItem::LargeTool(vertical::tool()),
                        RibbonItem::LargeTool(equal::tool()),
                        RibbonItem::LabeledDropdown {
                            id: "GCVISIBILITY", label: "Show/Hide",
                            icon: IconKind::Svg(include_bytes!("../../../assets/icons/constrain/show.svg")),
                            items: crate::modules::ribbon_command_items(&[
                                "GCSHOW", "GCHIDE", "GCRESET",
                            ]), default: "GCSHOW",
                        },
                        RibbonItem::LabeledTool(crate::modules::ribbon_command("GCSHOWALL")),
                        RibbonItem::LabeledTool(crate::modules::ribbon_command("GCHIDEALL")),
                    ],
                },
                RibbonGroup {
                    title: "Dimensional",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "DC_LINEAR_MENU", label: "Linear", icon: dimensional_tools::linear().icon,
                            items: crate::modules::ribbon_command_items(&[
                                "DCLINEAR",
                                "DCHORIZONTAL",
                                "DCVERTICAL",
                            ]),
                            default: "DCLINEAR",
                        },
                        RibbonItem::LargeTool(dimensional_tools::aligned()),
                        RibbonItem::LargeTool(dimensional_tools::angular()),
                        RibbonItem::LargeTool(dimensional_tools::diameter()),
                        RibbonItem::LargeTool(dimensional_tools::radius()),
                        RibbonItem::LargeTool(dimensional_tools::convert()),
                        RibbonItem::LabeledDropdown {
                            id: "DCVISIBILITY", label: "Show/Hide",
                            icon: IconKind::Svg(include_bytes!("../../../assets/icons/constrain/show.svg")),
                            items: crate::modules::ribbon_command_items(&["DCSHOW", "DCHIDE"]), default: "DCSHOW",
                        },
                        RibbonItem::LabeledTool(crate::modules::ribbon_command("DCSHOWALL")),
                        RibbonItem::LabeledTool(crate::modules::ribbon_command("DCHIDEALL")),
                    ],
                },
                RibbonGroup {
                    title: "Manage",
                    tools: vec![
                        RibbonItem::LargeTool(crate::modules::ribbon_command("DELCONSTRAINT")),
                        RibbonItem::LargeTool(crate::modules::ribbon_command("PARAMETERS")),
                    ],
                },
            ]
        })
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "AUTOCONSTRAIN", "CONSTRAINTSETTINGS", "GCSMOOTH", "GCSHOW", "GCHIDE", "GCRESET",
        "GCSHOWALL", "GCHIDEALL", "DCSHOW", "DCHIDE", "DCSHOWALL", "DCHIDEALL",
        "DCCONVERT", "DELCONSTRAINT",
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    fn item_id(item: &RibbonItem) -> &'static str {
        match item {
            RibbonItem::Tool(tool) | RibbonItem::LabeledTool(tool) | RibbonItem::LargeTool(tool) => tool.id,
            RibbonItem::Dropdown { id, .. } | RibbonItem::LabeledDropdown { id, .. }
            | RibbonItem::LargeDropdown { id, .. } => id,
            RibbonItem::ToolGrid { .. } => "GRID",
            _ => panic!("unexpected composite ribbon item"),
        }
    }

    #[test]
    fn ribbon_uses_geometry_dimension_and_manage_panels() {
        let groups = ParametricModule.ribbon_groups();

        assert_eq!(
            groups.iter().map(|group| group.title).collect::<Vec<_>>(),
            ["Geometric", "Dimensional", "Manage"]
        );
        assert_eq!(
            groups[0].tools.iter().map(item_id).collect::<Vec<_>>(),
            [
                "AUTOCONSTRAIN", "CCONSTRAINT", "PCONSTRAINT", "TCONSTRAINT",
                "LCONSTRAINT", "QCONSTRAINT", "GCSMOOTH", "GCCONCENTRIC",
                "GCHORIZONTAL", "SYCONSTRAINT", "FXCONSTRAINT", "VCONSTRAINT",
                "ECONSTRAINT", "GCVISIBILITY", "GCSHOWALL", "GCHIDEALL",
            ]
        );
        assert_eq!(
            groups[1].tools.iter().map(item_id).collect::<Vec<_>>(),
            [
                "DC_LINEAR_MENU", "DCALIGNED", "DCANGULAR", "DCDIAMETER",
                "DCRADIUS", "DCCONVERT", "DCVISIBILITY", "DCSHOWALL", "DCHIDEALL",
            ]
        );
        assert_eq!(groups[2].tools.iter().map(item_id).collect::<Vec<_>>(), ["DELCONSTRAINT", "PARAMETERS"]);

        let RibbonItem::LargeDropdown { items, default, .. } = &groups[1].tools[0] else {
            panic!("linear dimensional constraint must be a split dropdown");
        };
        assert_eq!(*default, "DCLINEAR");
        assert_eq!(
            items.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
            ["DCLINEAR", "DCHORIZONTAL", "DCVERTICAL"]
        );
        assert!(groups[0].tools[1..13]
            .iter()
            .all(|item| matches!(item, RibbonItem::LargeTool(_))));
        assert!(groups[1].tools[2..6]
            .iter()
            .all(|item| matches!(item, RibbonItem::LargeTool(_))));
    }
}
