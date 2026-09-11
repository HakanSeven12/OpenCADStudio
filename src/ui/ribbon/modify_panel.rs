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
        command: "OVERKILL",
        label: "Delete Duplicate Objects",
        icon: include_bytes!("../../../assets/icons/modify_overkill.svg"),
        options: &[],
    },
    Tool {
        command: "DRAWORDER_FRONT",
        label: "Draw Order",
        icon: include_bytes!("../../../assets/icons/modify_draworder.svg"),
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
