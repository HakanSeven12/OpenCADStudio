// The visual styles a viewport can be drawn in, in one list.
//
// Three places offer the same choice and used to describe it separately: the
// Visual Style dropdown on the ribbon, the render-mode picker with its preview
// cube, and the VISUALSTYLES command. The ribbon's copy had drifted furthest —
// it named four styles, two of which reached nothing (its "Shaded" carried the
// id `SOLID`, which is the 2D solid *drawing* command, and `HIDDEN` matched no
// command at all). Naming each style once, next to the command that applies it,
// is what keeps the three in step. (#621)

use codec::entities::ViewportRenderMode as Mode;

use crate::modules::IconKind;

const WIREFRAME_ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/wireframe.svg"));
const HIDDEN_ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/hidden.svg"));
const SHADED_ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/solid.svg"));

pub struct VisualStyle {
    pub mode: Mode,
    /// What the user sees, wherever the style is offered.
    pub label: &'static str,
    /// The command line that applies it. Ribbon items dispatch this verbatim,
    /// so it doubles as the item's identity for the checkmark and the
    /// last-used-tool memory. The keyword is the render mode's own name — there
    /// is one set of styles now, so there is one set of names for them.
    pub command: &'static str,
    pub icon: IconKind,
}

/// Every style, in the order they are offered — wireframes, then hidden line,
/// then the shaded ones, each pair plain before with-edges.
pub const VISUAL_STYLES: &[VisualStyle] = &[
    VisualStyle {
        mode: Mode::Wireframe2D,
        label: "Wireframe 2D",
        command: "VISUALSTYLES WIREFRAME2D",
        icon: WIREFRAME_ICON,
    },
    VisualStyle {
        mode: Mode::Wireframe3D,
        label: "Wireframe 3D",
        command: "VISUALSTYLES WIREFRAME3D",
        icon: WIREFRAME_ICON,
    },
    VisualStyle {
        mode: Mode::HiddenLine,
        label: "Hidden Line",
        command: "VISUALSTYLES HIDDENLINE",
        icon: HIDDEN_ICON,
    },
    VisualStyle {
        mode: Mode::FlatShaded,
        label: "Flat Shaded",
        command: "VISUALSTYLES FLATSHADED",
        icon: SHADED_ICON,
    },
    VisualStyle {
        mode: Mode::GouraudShaded,
        label: "Gouraud Shaded",
        command: "VISUALSTYLES GOURAUDSHADED",
        icon: SHADED_ICON,
    },
    VisualStyle {
        mode: Mode::FlatShadedWithEdges,
        label: "Flat Shaded + Edges",
        command: "VISUALSTYLES FLATSHADEDWITHEDGES",
        icon: SHADED_ICON,
    },
    VisualStyle {
        mode: Mode::GouraudShadedWithEdges,
        label: "Gouraud Shaded + Edges",
        command: "VISUALSTYLES GOURAUDSHADEDWITHEDGES",
        icon: SHADED_ICON,
    },
];

impl VisualStyle {
    /// The bare keyword, without the verb its `command` spells out.
    pub fn keyword(&self) -> &'static str {
        self.command
            .strip_prefix("VISUALSTYLES ")
            .unwrap_or(self.command)
    }
}

/// One-key answers to the style prompt (#2173): the digit or letter of the
/// common style names, each mapped to the mode that draws it.
pub const SHORT_KEYS: &[(&str, &str)] = &[
    ("2", "WIREFRAME2D"),
    ("3", "WIREFRAME3D"),
    ("W", "WIREFRAME3D"),
    ("H", "HIDDENLINE"),
    ("S", "GOURAUDSHADED"),
    ("R", "GOURAUDSHADED"),
    ("C", "GOURAUDSHADED"),
    ("E", "GOURAUDSHADEDWITHEDGES"),
];

/// The choices an interactive style prompt offers, in table order.
pub fn keyword_choices() -> Vec<(&'static str, &'static str, Option<&'static str>)> {
    VISUAL_STYLES
        .iter()
        .map(|style| (style.label, style.keyword(), None))
        .collect()
}

/// Stable source used by the shared command prompt translator.
pub fn keyword_prompt() -> &'static str {
    "Visual style [Wireframe 2D/Wireframe 3D/Hidden Line/Flat Shaded/Gouraud Shaded/Flat Shaded + Edges/Gouraud Shaded + Edges]:"
}

pub fn label_for(mode: Mode) -> &'static str {
    VISUAL_STYLES
        .iter()
        .find(|style| style.mode == mode)
        .map(|style| style.label)
        .unwrap_or("Wireframe 2D")
}

/// The render mode a named visual style draws as, and whether the name is a
/// style of its own (Realistic, Conceptual, …) rather than one of the modes.
pub fn mode_for_style_name(name: &str) -> Option<(Mode, bool)> {
    Some(match name {
        "2dWireframe" => (Mode::Wireframe2D, false),
        "Wireframe" => (Mode::Wireframe3D, false),
        "Hidden" => (Mode::HiddenLine, false),
        "Flat" => (Mode::FlatShaded, false),
        "FlatWithEdges" => (Mode::FlatShadedWithEdges, false),
        "Gouraud" => (Mode::GouraudShaded, false),
        "GouraudWithEdges" => (Mode::GouraudShadedWithEdges, false),
        "Shaded with edges" => (Mode::GouraudShadedWithEdges, true),
        "Realistic" | "Shaded" | "Conceptual" | "Shades of Gray" | "Sketchy" | "X-Ray" => {
            (Mode::GouraudShaded, true)
        }
        _ => return None,
    })
}

/// The name of the visual style a render mode is saved as.
pub fn style_name_for(mode: Mode) -> &'static str {
    match mode {
        Mode::Wireframe2D => "2dWireframe",
        Mode::Wireframe3D => "Wireframe",
        Mode::HiddenLine => "Hidden",
        Mode::FlatShaded => "Flat",
        Mode::FlatShadedWithEdges => "FlatWithEdges",
        Mode::GouraudShaded => "Gouraud",
        Mode::GouraudShadedWithEdges => "GouraudWithEdges",
    }
}

/// The style a style keyword names. Only the seven exist; nothing maps onto a
/// nearest neighbour, because there is no longer anything else to map from.
pub fn mode_for_keyword(keyword: &str) -> Option<Mode> {
    let keyword = keyword.trim().to_uppercase();
    let keyword = SHORT_KEYS
        .iter()
        .find(|(key, _)| *key == keyword)
        .map_or(keyword.as_str(), |(_, keyword)| keyword);
    VISUAL_STYLES
        .iter()
        .find(|style| style.keyword() == keyword)
        .map(|style| style.mode)
}
