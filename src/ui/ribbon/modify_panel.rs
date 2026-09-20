use super::draw_panel::Tool;

pub(super) const TOOLS: &[Tool] = &[
    Tool {
        command: "SETBYLAYER",
        label: "Set to ByLayer",
        icon: include_bytes!("../../../assets/icons/modify_setbylayer.svg"),
        options: &[],
    },
    Tool {
        command: "LENGTHEN",
        label: "Lengthen",
        icon: include_bytes!("../../../assets/icons/modify_lengthen.svg"),
        options: &[],
    },
    Tool {
        command: "PEDIT",
        label: "Edit Polyline",
        icon: include_bytes!("../../../assets/icons/modify_pedit.svg"),
        options: &[],
    },
    Tool {
        command: "SPLINEDIT",
        label: "Edit Spline",
        icon: include_bytes!("../../../assets/icons/modify_splinedit.svg"),
        options: &[],
    },
    Tool {
        command: "HATCHEDIT",
        label: "Edit Hatch",
        icon: include_bytes!("../../../assets/icons/modify_hatchedit.svg"),
        options: &[],
    },
    Tool {
        command: "ALIGN",
        label: "Align",
        icon: include_bytes!("../../../assets/icons/modify_align.svg"),
        options: &[],
    },
    // ALIGNLEFT/ALIGNHCENTER/ALIGNRIGHT/ALIGNTOP/ALIGNVCENTER/ALIGNBOTTOM:
    // one-shot bounding-box alignment of the current selection — distinct
    // from "ALIGN" above (point-picked 3D placement). No point prompts, so
    // each is a plain one-click Tool rather than a dropdown submenu, matching
    // this panel's existing flat-list convention.
    Tool {
        command: "ALIGNLEFT",
        label: "Align Left",
        icon: include_bytes!("../../../assets/icons/align_left.svg"),
        options: &[],
    },
    Tool {
        command: "ALIGNHCENTER",
        label: "Align Horizontal Centers",
        icon: include_bytes!("../../../assets/icons/align_hcenter.svg"),
        options: &[],
    },
    Tool {
        command: "ALIGNRIGHT",
        label: "Align Right",
        icon: include_bytes!("../../../assets/icons/align_right.svg"),
        options: &[],
    },
    Tool {
        command: "ALIGNTOP",
        label: "Align Top",
        icon: include_bytes!("../../../assets/icons/align_top.svg"),
        options: &[],
    },
    Tool {
        command: "ALIGNVCENTER",
        label: "Align Vertical Centers",
        icon: include_bytes!("../../../assets/icons/align_vcenter.svg"),
        options: &[],
    },
    Tool {
        command: "ALIGNBOTTOM",
        label: "Align Bottom",
        icon: include_bytes!("../../../assets/icons/align_bottom.svg"),
        options: &[],
    },
    Tool {
        command: "BREAK",
        label: "Break",
        icon: include_bytes!("../../../assets/icons/modify_break.svg"),
        options: &[],
    },
    Tool {
        command: "BREAKATPOINT",
        label: "Break at Point",
        icon: include_bytes!("../../../assets/icons/modify_breakatpoint.svg"),
        options: &[],
    },
    Tool {
        command: "JOIN",
        label: "Join",
        icon: include_bytes!("../../../assets/icons/modify_join.svg"),
        options: &[],
    },
    Tool {
        command: "REVERSE",
        label: "Reverse",
        icon: include_bytes!("../../../assets/icons/modify_reverse.svg"),
        options: &[],
    },
    Tool {
        command: "NCOPY",
        label: "Copy Nested Objects",
        icon: include_bytes!("../../../assets/icons/modify_ncopy.svg"),
        options: &[],
    },
    Tool {
        command: "DRAWORDER_FRONT",
        label: "Draw Order",
        icon: crate::ui::icon_catalog::bytes(crate::ui::icon_catalog::IconId::DrawOrder),
        options: &[
            ("DRAWORDER_FRONT", "Bring to Front"),
            ("DRAWORDER_BACK", "Send to Back"),
            ("DRAWORDER_ABOVE", "Bring Above Objects"),
            ("DRAWORDER_UNDER", "Send Under Objects"),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::TOOLS;

    #[test]
    fn every_tool_command_is_registered() {
        let registered = crate::command::all_registered_command_names();
        for command in TOOLS
            .iter()
            .flat_map(|tool| std::iter::once(tool.command).chain(tool.options.iter().map(|o| o.0)))
        {
            assert!(registered.contains(&command), "{command} is not registered");
        }
    }

    #[test]
    fn split_tools_follow_plain_tools() {
        let first_split = TOOLS
            .iter()
            .position(|tool| !tool.options.is_empty())
            .unwrap_or(TOOLS.len());
        assert!(TOOLS[..first_split]
            .iter()
            .all(|tool| tool.options.is_empty()));
        assert!(TOOLS[first_split..]
            .iter()
            .all(|tool| !tool.options.is_empty()));
    }
}
