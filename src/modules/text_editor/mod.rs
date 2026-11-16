//! Contextual "Text Editor" ribbon tab, shown while a multiline text is edited
//! in place: style, character formatting, paragraphs, inserts, spelling,
//! tools and editor options. Every tool runs `_MTED <action>`, which the app
//! hands to the open editor. The Style and Formatting panels draw their lists
//! and number boxes from the editor itself (`ui::ribbon::text_editor`).

use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct TextEditorModule;

/// The tab's module id.
pub const ID: &str = "text_editor";

const fn svg(bytes: &'static [u8]) -> IconKind {
    IconKind::Svg(bytes)
}

fn tool(id: &'static str, label: &'static str, icon: IconKind) -> ToolDef {
    ToolDef {
        id,
        label,
        icon,
        event: ModuleEvent::Command(id.to_string()),
    }
}

/// The nine attachment points, as the Justification menu lists them.
fn justifications() -> Vec<(&'static str, &'static str, IconKind)> {
    macro_rules! j {
        ($code:literal, $label:literal, $icon:literal) => {
            (
                concat!("_MTED JUSTIFY ", $code),
                $label,
                svg(include_bytes!(concat!(
                    "../../../assets/icons/table_align_",
                    $icon,
                    ".svg"
                ))),
            )
        };
    }
    vec![
        j!("TL", "Top Left TL", "tl"),
        j!("TC", "Top Center TC", "tc"),
        j!("TR", "Top Right TR", "tr"),
        j!("ML", "Middle Left ML", "ml"),
        j!("MC", "Middle Center MC", "mc"),
        j!("MR", "Middle Right MR", "mr"),
        j!("BL", "Bottom Left BL", "bl"),
        j!("BC", "Bottom Center BC", "bc"),
        j!("BR", "Bottom Right BR", "br"),
    ]
}

impl CadModule for TextEditorModule {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Text Editor"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            let glyph = IconKind::Glyph;
            let small = |id, label, icon| RibbonItem::LabeledTool(tool(id, label, icon));
            let large = |id, label, icon| RibbonItem::LargeTool(tool(id, label, icon));
            vec![
                // Lists, the annotative switch, the height box and Mask: drawn
                // by the host from the open editor.
                // The style list and the height box follow the buttons; the
                // host draws them from the open editor.
                RibbonGroup {
                    title: "Style",
                    tools: vec![
                        small(
                            "_MTED ANNOTATIVE",
                            "Annotative",
                            svg(include_bytes!("../../../assets/icons/text_style.svg")),
                        ),
                        small(
                            "_MTED MASK",
                            "Mask",
                            svg(include_bytes!("../../../assets/icons/wipeout.svg")),
                        ),
                    ],
                },
                // The font, colour and layer lists follow the buttons.
                RibbonGroup {
                    title: "Formatting",
                    tools: vec![
                        large(
                            "_MTED MATCH",
                            "Match",
                            svg(include_bytes!("../../../assets/icons/match_prop.svg")),
                        ),
                        RibbonItem::ToolGrid {
                            columns: vec![
                                vec![
                                    tool("_MTED BOLD", "Bold", svg(include_bytes!("../../../assets/icons/mt_bold.svg"))),
                                    tool("_MTED UNDERLINE", "Underline", svg(include_bytes!("../../../assets/icons/mt_underline.svg"))),
                                    tool("_MTED SUPERSCRIPT", "Superscript", svg(include_bytes!("../../../assets/icons/mt_superscript.svg"))),
                                ],
                                vec![
                                    tool("_MTED ITALIC", "Italic", svg(include_bytes!("../../../assets/icons/mt_italic.svg"))),
                                    tool("_MTED OVERLINE", "Overline", svg(include_bytes!("../../../assets/icons/mt_overline.svg"))),
                                    tool("_MTED SUBSCRIPT", "Subscript", svg(include_bytes!("../../../assets/icons/mt_subscript.svg"))),
                                ],
                                vec![
                                    tool("_MTED STRIKE", "Strikethrough", svg(include_bytes!("../../../assets/icons/mt_strike.svg"))),
                                    tool("_MTED STACK", "Stack", glyph("½")),
                                ],
                            ],
                        },
                        RibbonItem::Dropdown {
                            id: "MTED_CLEAR",
                            icon: svg(include_bytes!("../../../assets/icons/mt_cancel.svg")),
                            items: vec![
                                (
                                    "_MTED CLEAR CHAR",
                                    "Remove Character Formatting",
                                    svg(include_bytes!("../../../assets/icons/mt_cancel.svg")),
                                ),
                                (
                                    "_MTED CLEAR PARA",
                                    "Remove Paragraph Formatting",
                                    svg(include_bytes!("../../../assets/icons/mt_cancel.svg")),
                                ),
                                (
                                    "_MTED CLEAR ALL",
                                    "Remove All Formatting",
                                    svg(include_bytes!("../../../assets/icons/mt_cancel.svg")),
                                ),
                            ],
                            default: "_MTED CLEAR CHAR",
                        },
                        RibbonItem::Dropdown {
                            id: "MTED_CASE",
                            icon: svg(include_bytes!("../../../assets/icons/mt_upper.svg")),
                            items: vec![
                                (
                                    "_MTED CASE UPPER",
                                    "UPPERCASE",
                                    svg(include_bytes!("../../../assets/icons/mt_upper.svg")),
                                ),
                                (
                                    "_MTED CASE LOWER",
                                    "lowercase",
                                    svg(include_bytes!("../../../assets/icons/mt_lower.svg")),
                                ),
                            ],
                            default: "_MTED CASE UPPER",
                        },
                    ],
                },
                RibbonGroup {
                    title: "Paragraph",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "MTED_JUSTIFY",
                            label: "Justification",
                            icon: svg(include_bytes!("../../../assets/icons/table_align_tl.svg")),
                            items: justifications(),
                            default: "_MTED JUSTIFY TL",
                        },
                        RibbonItem::LabeledDropdown {
                            id: "MTED_BULLETS",
                            label: "Bullets and Numbering",
                            icon: glyph("•"),
                            items: vec![
                                ("_MTED LIST OFF", "Off", glyph("")),
                                ("_MTED LIST NUMBER", "Numbered", glyph("1.")),
                                ("_MTED LIST LOWER", "Lettered Lowercase", glyph("a.")),
                                ("_MTED LIST UPPER", "Lettered Uppercase", glyph("A.")),
                                ("_MTED LIST BULLET", "Bulleted", glyph("•")),
                                ("_MTED LIST START", "Start", glyph("")),
                                ("_MTED LIST CONTINUE", "Continue", glyph("")),
                                (
                                    "_MTED LIST AUTO",
                                    "Allow Auto Bullets and Numbering",
                                    glyph(""),
                                ),
                                ("_MTED LIST ALLOW", "Allow Bullets and Lists", glyph("")),
                            ],
                            default: "_MTED LIST BULLET",
                        },
                        RibbonItem::LabeledDropdown {
                            id: "MTED_LINESPACING",
                            label: "Line Spacing",
                            icon: glyph("≡"),
                            items: vec![
                                ("_MTED SPACING 1", "1.0x", glyph("")),
                                ("_MTED SPACING 1.5", "1.5x", glyph("")),
                                ("_MTED SPACING 2", "2.0x", glyph("")),
                                ("_MTED SPACING 2.5", "2.5x", glyph("")),
                                ("_MTED PARAGRAPH", "More...", glyph("")),
                                ("_MTED SPACING CLEAR", "Clear Line Space", glyph("")),
                            ],
                            default: "_MTED SPACING 1",
                        },
                        RibbonItem::ToolGrid {
                            columns: vec![
                                vec![tool(
                                    "_MTED ALIGN DEFAULT",
                                    "Default",
                                    svg(include_bytes!("../../../assets/icons/mt_align_left.svg")),
                                )],
                                vec![tool(
                                    "_MTED ALIGN LEFT",
                                    "Left",
                                    svg(include_bytes!("../../../assets/icons/mt_align_left.svg")),
                                )],
                                vec![tool(
                                    "_MTED ALIGN CENTER",
                                    "Center",
                                    svg(include_bytes!(
                                        "../../../assets/icons/mt_align_center.svg"
                                    )),
                                )],
                                vec![tool(
                                    "_MTED ALIGN RIGHT",
                                    "Right",
                                    svg(include_bytes!(
                                        "../../../assets/icons/mt_align_right.svg"
                                    )),
                                )],
                                vec![tool(
                                    "_MTED ALIGN JUSTIFY",
                                    "Justify",
                                    svg(include_bytes!(
                                        "../../../assets/icons/mt_align_justify.svg"
                                    )),
                                )],
                                vec![tool(
                                    "_MTED ALIGN DISTRIBUTE",
                                    "Distribute",
                                    glyph("↔"),
                                )],
                            ],
                        },
                    ],
                },
                RibbonGroup {
                    title: "Insert",
                    tools: vec![
                        RibbonItem::LargeDropdown {
                            id: "MTED_COLUMNS",
                            label: "Columns",
                            icon: svg(include_bytes!("../../../assets/icons/table_col_right.svg")),
                            items: vec![
                                ("_MTED COLUMNS NONE", "No Columns", glyph("")),
                                ("_MTED COLUMNS DYNAMIC AUTO", "Dynamic Columns: Auto height", glyph("")),
                                (
                                    "_MTED COLUMNS DYNAMIC MANUAL",
                                    "Dynamic Columns: Manual height",
                                    glyph(""),
                                ),
                                ("_MTED COLUMNS STATIC 2", "Static Columns: 2", glyph("")),
                                ("_MTED COLUMNS STATIC 3", "Static Columns: 3", glyph("")),
                                ("_MTED COLUMNS STATIC 4", "Static Columns: 4", glyph("")),
                                ("_MTED COLUMNS STATIC 5", "Static Columns: 5", glyph("")),
                                ("_MTED COLUMNS STATIC 6", "Static Columns: 6", glyph("")),
                                ("_MTED COLUMNBREAK", "Insert Column Break Alt+Enter", glyph("")),
                                ("_MTED COLUMNSETTINGS", "Column Settings...", glyph("")),
                            ],
                            default: "_MTED COLUMNSETTINGS",
                        },
                        RibbonItem::LargeDropdown {
                            id: "MTED_SYMBOL",
                            label: "Symbol",
                            icon: glyph("Ω"),
                            items: symbols(),
                            default: "_MTED SYMBOL %%d",
                        },
                        large(
                            "_MTED FIELD",
                            "Field",
                            svg(include_bytes!("../../../assets/icons/text.svg")),
                        ),
                    ],
                },
                RibbonGroup {
                    title: "Spell Check",
                    tools: vec![
                        large("_MTED SPELL", "Spell\nCheck", glyph("ABC")),
                        large("_MTED DICTIONARIES", "Edit\nDictionaries", glyph("Aa")),
                    ],
                },
                RibbonGroup {
                    title: "Tools",
                    tools: vec![
                        large(
                            "_MTED FIND",
                            "Find &\nReplace",
                            svg(include_bytes!("../../../assets/icons/find.svg")),
                        ),
                        small(
                            "_MTED IMPORT",
                            "Import Text",
                            svg(include_bytes!("../../../assets/icons/ui/import.svg")),
                        ),
                        small(
                            "_MTED AUTOCAPS",
                            "All CAPS",
                            svg(include_bytes!("../../../assets/icons/mt_upper.svg")),
                        ),
                    ],
                },
                RibbonGroup {
                    title: "Options",
                    tools: vec![
                        RibbonItem::LabeledDropdown {
                            id: "MTED_MORE",
                            label: "More",
                            icon: glyph("☰"),
                            items: more_items(),
                            default: "_MTED CHARSET",
                        },
                        small("_MTED RULER", "Ruler", glyph("⊢")),
                        RibbonItem::ToolGrid {
                            columns: vec![
                                vec![tool(
                                    "_MTED UNDO",
                                    "Undo",
                                    svg(include_bytes!("../../../assets/icons/ui/undo.svg")),
                                )],
                                vec![tool(
                                    "_MTED REDO",
                                    "Redo",
                                    svg(include_bytes!("../../../assets/icons/ui/redo.svg")),
                                )],
                            ],
                        },
                    ],
                },
                RibbonGroup {
                    title: "Close",
                    tools: vec![large(
                        "_MTED CLOSE",
                        "Close\nText Editor",
                        svg(include_bytes!("../../../assets/icons/mt_ok.svg")),
                    )],
                },
            ]
        })
    }
}

/// The Symbol menu: each inserts its text (`%%d`, a Unicode character, or a
/// non-breaking space); Other… opens the character map.
pub fn symbols() -> Vec<(&'static str, &'static str, IconKind)> {
    let glyph = IconKind::Glyph;
    vec![
        ("_MTED SYMBOL %%d", "Degrees   %%d", glyph("°")),
        ("_MTED SYMBOL %%p", "Plus/Minus   %%p", glyph("±")),
        ("_MTED SYMBOL %%c", "Diameter   %%c", glyph("∅")),
        ("_MTED SYMBOL \\U+2248", "Almost Equal   \\U+2248", glyph("≈")),
        ("_MTED SYMBOL \\U+2220", "Angle   \\U+2220", glyph("∠")),
        ("_MTED SYMBOL \\U+E100", "Boundary Line   \\U+E100", glyph("")),
        ("_MTED SYMBOL \\U+2104", "Center Line   \\U+2104", glyph("℄")),
        ("_MTED SYMBOL \\U+0394", "Delta   \\U+0394", glyph("Δ")),
        ("_MTED SYMBOL \\U+0278", "Electrical Phase   \\U+0278", glyph("ɸ")),
        ("_MTED SYMBOL \\U+E101", "Flow Line   \\U+E101", glyph("")),
        ("_MTED SYMBOL \\U+2261", "Identity   \\U+2261", glyph("≡")),
        ("_MTED SYMBOL \\U+E200", "Initial Length   \\U+E200", glyph("")),
        ("_MTED SYMBOL \\U+E102", "Monument Line   \\U+E102", glyph("")),
        ("_MTED SYMBOL \\U+2260", "Not Equal   \\U+2260", glyph("≠")),
        ("_MTED SYMBOL \\U+2126", "Ohm   \\U+2126", glyph("Ω")),
        ("_MTED SYMBOL \\U+03A9", "Omega   \\U+03A9", glyph("Ω")),
        ("_MTED SYMBOL \\U+214A", "Property Line   \\U+214A", glyph("⅊")),
        ("_MTED SYMBOL \\U+2082", "Subscript 2   \\U+2082", glyph("₂")),
        ("_MTED SYMBOL \\U+00B2", "Squared   \\U+00B2", glyph("²")),
        ("_MTED SYMBOL \\U+00B3", "Cubed   \\U+00B3", glyph("³")),
        ("_MTED SYMBOL NBSP", "Non-breaking Space Ctrl+Shift+Space", glyph("")),
        ("_MTED CHARMAP", "Other...", glyph("")),
    ]
}

/// The Options > More menu: character sets, the editor settings, the stack
/// windows and help. A setting that is on shows a check mark.
fn more_items() -> Vec<(&'static str, &'static str, IconKind)> {
    let glyph = IconKind::Glyph;
    vec![
        ("_MTED CHARSET", "Character Set...", glyph("")),
        ("_MTED WYSIWYG", "Always Display as WYSIWYG", glyph("")),
        ("_MTED TOOLBAR", "Show Toolbar", glyph("")),
        ("_MTED BACKGROUND", "Show Background", glyph("")),
        ("_MTED HIGHLIGHT", "Text Highlight Color...", glyph("")),
        ("_MTED STACKPROPS", "Stack Properties...", glyph("½")),
        ("_MTED AUTOSTACK", "AutoStack...", glyph("")),
        ("_MTED HELP", "Help", glyph("?")),
    ]
}
