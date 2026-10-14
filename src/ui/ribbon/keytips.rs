//! Key tips: a lone Alt shows a code on every ribbon tab and quick-access
//! button; typing a tab's code opens it and shows the codes of its buttons.
//! A button runs, a dropdown or slide-out opens and shows the codes of its
//! items, and Esc goes back one level. The codes come from the reference
//! ribbon; buttons it has no counterpart for get none.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use iced::advanced::layout::{self, Layout};
use iced::advanced::text::{self as advanced_text, Paragraph as _, Renderer as _};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Renderer as _};
use iced::keyboard::{self, key::Named, Key};
use iced::{
    Background, Border, Color, Element, Length, Pixels, Point, Rectangle, Renderer, Shadow, Size,
    Theme, Vector,
};

use super::{draw_panel, item_id, Ribbon};
use crate::app::Message;
use crate::modules::{RibbonItem, ToolDef};
use crate::ui::wrap_bar::{fresh_bounds, PosReport};

/// Key tip input, from the keyboard listener or automation.
#[derive(Debug, Clone)]
pub enum KeyTipInput {
    /// Alt was pressed and released on its own.
    AltTap,
    /// Show the tab level; `refocus` returns the focus to the command line
    /// when the tips close.
    Open { refocus: bool },
    /// One typed character (upper case).
    Char(String),
    /// Settle a code that also starts a longer one.
    Enter,
    Escape,
    Hide,
}

/// What a typed code leads to.
pub enum KeyTipStep {
    /// Nothing matched; the key is ignored.
    Ignore,
    /// A partial code, or a tab that is now open.
    Stay,
    /// A dropdown, slide-out or collapsed panel opens; the tips stay.
    Open(Message),
    /// A command runs and the tips close.
    Run(Message),
}

#[derive(Clone)]
enum Action {
    Tab(usize),
    Open(Message),
    Run(Message),
}

/// One code shown on one widget.
pub struct Tip {
    /// The id the widget reports its bounds under.
    pub anchor: String,
    /// The table key: tool id, dropdown id, group title or command.
    pub target: &'static str,
    pub code: Option<String>,
    action: Action,
}

/// Open key tips.
pub(super) struct State {
    /// Showing the tab codes rather than the open tab's buttons.
    top: bool,
    /// Codes of the levels entered so far.
    path: Vec<String>,
    /// The start of a longer code.
    typed: String,
    /// The command line had the focus when the tips opened.
    refocus: bool,
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Alt is down and nothing else was pressed since.
static ALT_ALONE: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Anchors that carried a code in the last drawn frame.
    static SHOWN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Id of the whole ribbon (tab row and tools), for the hint bar.
const RIBBON_AREA: &str = "KEYTIP_RIBBON_AREA";
/// The Reference panel's corner arrow.
pub(super) const XREF_LAUNCHER: &str = "XREF_LAUNCHER";
/// The xref fading switch in the Reference slide-out.
pub(super) const XREF_FADE: &str = "XREF_FADE";
const COLLAPSED_PREFIX: &str = "panel:";

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Record `el`'s bounds under `id` while key tips show.
pub(super) fn anchor<'a>(id: &'static str, el: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    if active() {
        PosReport::new(id, el).into()
    } else {
        el.into()
    }
}

/// `anchor` under an id built only while key tips show.
pub(super) fn anchor_with<'a>(
    id: impl FnOnce() -> String,
    el: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    if active() {
        PosReport::owned(id(), el).into()
    } else {
        el.into()
    }
}

/// Anchor id of item `cmd` in dropdown or slide-out `dropdown`.
pub(super) fn item_anchor(dropdown: &str, cmd: &str) -> String {
    format!("{dropdown}/{cmd}")
}

/// Anchor id of a quick-access button (its command can also be a tool's).
pub(super) fn quick_access_anchor(id: &str) -> String {
    if id == super::widgets::UNDO_HISTORY_ID || id == super::widgets::REDO_HISTORY_ID {
        id.to_string()
    } else {
        format!("qa:{id}")
    }
}

/// Anchor id of a collapsed panel's button.
pub(super) fn collapsed_anchor(title: &str) -> String {
    format!("{COLLAPSED_PREFIX}{title}")
}

/// Keyboard listener hook for a key press. `Some` while key tips own the
/// keyboard (the inner value is the message to send, if any).
pub fn key_pressed(key: &Key, modifiers: keyboard::Modifiers, repeat: bool) -> Option<Option<Message>> {
    let alt = matches!(key, Key::Named(Named::Alt));
    if !repeat {
        ALT_ALONE.store(
            alt && !modifiers.control() && !modifiers.shift() && !modifiers.logo(),
            Ordering::Relaxed,
        );
    }
    if !active() {
        return None;
    }
    Some(match key {
        Key::Named(Named::Escape) => Some(Message::KeyTip(KeyTipInput::Escape)),
        Key::Named(Named::Enter) => Some(Message::KeyTip(KeyTipInput::Enter)),
        Key::Character(value) if !modifiers.control() && !modifiers.alt() => value
            .chars()
            .next()
            .filter(|ch| ch.is_alphanumeric())
            // A dotted capital I (Turkish layouts) types the code letter I.
            .map(|ch| if ch == 'İ' { 'I' } else { ch })
            .map(|ch| Message::KeyTip(KeyTipInput::Char(ch.to_uppercase().collect()))),
        _ => None,
    })
}

/// Keyboard listener hook for a key release: a lone Alt toggles the tips.
pub fn key_released(key: &Key) -> Option<Message> {
    (matches!(key, Key::Named(Named::Alt)) && ALT_ALONE.swap(false, Ordering::Relaxed))
        .then_some(Message::KeyTip(KeyTipInput::AltTap))
}

/// A pointer press ends a lone Alt.
pub fn pointer_pressed() {
    ALT_ALONE.store(false, Ordering::Relaxed);
}

/// Codes of the ribbon tabs, by module id.
fn tab_code(module: &str) -> Option<&'static str> {
    Some(match module {
        "draw" => "H",
        "parametric" => "PA",
        "model" => "3D",
        "insert" => "IN",
        "annotate" => "AN",
        "view" => "VI",
        "manage" => "MA",
        "block_editor" => "BE",
        _ => return None,
    })
}

/// Quick-access buttons in order; their codes are their positions.
const QUICK_ACCESS: &[&str] = &[
    "QNEW",
    "OPEN",
    "SAVE",
    "SAVEAS",
    "PRINT",
    super::widgets::UNDO_HISTORY_ID,
    super::widgets::REDO_HISTORY_ID,
];

/// Reference codes as (scope, target, code). In a tab's scope (its module id)
/// the target is a tool id, a dropdown id or a group title; a group title's
/// code opens the panel's slide-out or, while the panel is collapsed, its
/// flyout. In a dropdown's or slide-out's scope the target is an item's
/// command.
const CODES: &[(&str, &str, &str)] = &[
    // Draw (the reference Home tab).
    ("draw", "Draw", "DW"),
    ("draw", "LINE", "LI"),
    ("draw", "PLINE", "PL"),
    ("draw", "CIRCLE", "CI"),
    ("draw", "ARC", "AR"),
    ("draw", "SHAPES", "RE"),
    ("draw", "ELLIPSE", "EL"),
    ("draw", "HATCH", "HA"),
    ("SHAPES", "RECT", "RE"),
    ("SHAPES", "POLY", "PO"),
    ("HATCH", "HATCH", "HA"),
    ("HATCH", "GRADIENT", "GR"),
    ("HATCH", "BOUNDARY", "BO"),
    ("draw_extension", "SPLINE", "SF"),
    ("draw_extension", "SPLINECV", "SC"),
    ("draw_extension", "XLINE", "XL"),
    ("draw_extension", "RAY", "RA"),
    ("draw_extension", "DIVIDE", "DI"),
    ("draw_extension", "MEASURE", "ME"),
    ("draw_extension", "REGION", "RG"),
    ("draw_extension", "HELIX", "HE"),
    ("draw_extension", "DONUT", "DO"),
    ("draw_extension", "MULTIPOINT", "MP"),
    ("draw", "Modify", "MD"),
    ("draw", "MOVE", "MO"),
    ("draw", "COPY", "CO"),
    ("draw", "STRETCH", "ST"),
    ("draw", "ROTATE", "RO"),
    ("draw", "MIRROR", "MI"),
    ("draw", "SCALE", "SC"),
    ("draw", "trim_extend", "TR"),
    ("draw", "fillet_chamfer", "FI"),
    ("draw", "array_type", "AY"),
    ("draw", "ERASE", "ER"),
    ("draw", "EXPLODE", "EX"),
    ("draw", "OFFSET", "OF"),
    ("modify_extension", "SETBYLAYER", "SL"),
    ("modify_extension", "LENGTHEN", "LE"),
    ("modify_extension", "PEDIT", "EP"),
    ("modify_extension", "SPLINEDIT", "ES"),
    ("modify_extension", "HATCHEDIT", "EH"),
    ("modify_extension", "ALIGN", "AL"),
    ("modify_extension", "BREAK", "BR"),
    ("modify_extension", "BREAKATPOINT", "BP"),
    ("modify_extension", "JOIN", "JO"),
    ("modify_extension", "REVERSE", "RV"),
    ("modify_extension", "NCOPY", "CN"),
    ("modify_extension", "DRAWORDER_FRONT", "DR"),
    ("draw", "Annotation", "AN"),
    ("draw", "ANNOTATION_TEXT", "MT"),
    ("draw", "ANNOTATION_DIMENSIONS", "DI"),
    ("draw", "ANNOTATION_LEADER", "ML"),
    ("draw", "Layers", "LL"),
    ("draw", "LAYERS", "LP"),
    ("draw", "LAYER_COMBO", "LA"),
    ("draw", "LAYOFF", "LO"),
    ("draw", "LAYFRZ", "LF"),
    ("draw", "LAYLCK", "LK"),
    ("draw", "LAYMCUR", "LC"),
    ("draw", "LAYISO", "LS"),
    ("draw", "LAYON", "LN"),
    ("draw", "LAYTHW", "LW"),
    ("draw", "LAYULK", "UL"),
    ("draw", "LAYMATCH", "LM"),
    ("draw", "LAYUNISO", "LU"),
    ("draw", "Block", "BK"),
    ("draw", "BLOCK", "BC"),
    ("draw", "BEDIT", "BE"),
    ("draw", "ATTEDIT", "AE"),
    ("home_block_extension", "ATTDEF", "AD"),
    ("home_block_extension", "ATTMAN", "AM"),
    ("home_block_extension", "ATTSYNC", "AS"),
    ("home_block_extension", "BASE", "BA"),
    ("draw", "Properties", "PE"),
    ("draw", "PROP_LINETYPE", "PY"),
    ("draw", "PROP_LW", "PW"),
    ("draw", "Groups", "GR"),
    ("draw", "GROUP", "GG"),
    ("draw", "UNGROUP", "GU"),
    ("draw", "PASTE_MENU", "PA"),
    ("draw", "COPYCLIP", "CC"),
    ("draw", "CUTCLIP", "CX"),
    ("draw", "MEASURE_MENU", "DT"),
    // Parametric.
    ("parametric", "AUTOCONSTRAIN", "AC"),
    ("parametric", "CCONSTRAINT", "CI"),
    ("parametric", "PCONSTRAINT", "PA"),
    ("parametric", "TCONSTRAINT", "TA"),
    ("parametric", "LCONSTRAINT", "CL"),
    ("parametric", "QCONSTRAINT", "PE"),
    ("parametric", "GCSMOOTH", "SM"),
    ("parametric", "GCCONCENTRIC", "CO"),
    ("parametric", "GCHORIZONTAL", "HO"),
    ("parametric", "SYCONSTRAINT", "SY"),
    ("parametric", "FXCONSTRAINT", "FI"),
    ("parametric", "VCONSTRAINT", "VE"),
    ("parametric", "ECONSTRAINT", "EQ"),
    ("parametric", "GCVISIBILITY", "SH"),
    ("parametric", "GCSHOWALL", "SA"),
    ("parametric", "GCHIDEALL", "HA"),
    ("parametric", "Dimensional", "DM"),
    ("parametric", "DC_LINEAR_MENU", "DC"),
    ("parametric", "DCALIGNED", "AL"),
    ("parametric", "DCRADIUS", "RA"),
    ("parametric", "DCDIAMETER", "DI"),
    ("parametric", "DCANGULAR", "AN"),
    ("parametric", "DCCONVERT", "DO"),
    ("parametric", "DCVISIBILITY", "TD"),
    ("parametric", "DCSHOWALL", "SD"),
    ("parametric", "DCHIDEALL", "HD"),
    ("parametric", "DELCONSTRAINT", "DE"),
    ("parametric", "PARAMETERS", "PM"),
    // Model (the reference Solid tab).
    ("model", "MODEL_PRIMITIVES", "SS"),
    ("MODEL_PRIMITIVES", "BOX", "BO"),
    ("MODEL_PRIMITIVES", "CYLINDER", "CY"),
    ("MODEL_PRIMITIVES", "CONE", "CO"),
    ("MODEL_PRIMITIVES", "SPHERE", "SP"),
    ("MODEL_PRIMITIVES", "PYRAMID", "PY"),
    ("MODEL_PRIMITIVES", "WEDGE", "WE"),
    ("MODEL_PRIMITIVES", "TORUS", "TO"),
    ("MODEL_PRIMITIVES", "POLYSOLID", "SR"),
    ("model", "EXTRUDE", "EX"),
    ("model", "REVOLVE", "RV"),
    ("model", "LOFT", "LO"),
    ("model", "SWEEP", "SY"),
    ("model", "PRESSPULL", "PU"),
    ("model", "UNION", "UN"),
    ("model", "SUBTRACT", "SU"),
    ("model", "INTERSECT", "IN"),
    ("model", "FILLETEDGE", "ED"),
    ("model", "CHAMFEREDGE", "CE"),
    ("model", "SHELL", "SH"),
    // Insert.
    ("insert", "Block", "BK"),
    ("insert", "INSERT_GALLERY", "BI"),
    ("insert", "ATTEDIT_DROPDOWN", "AE"),
    ("insert", "ATTMODE_DROPDOWN", "AT"),
    ("insert", "Block Definition", "BD"),
    ("insert", "BLOCK", "BC"),
    ("insert", "ATTDEF", "AD"),
    ("insert", "ATTMAN", "AM"),
    ("insert", "BEDIT", "BE"),
    ("block_definition_extension", "BASE", "BA"),
    ("block_definition_extension", "ATTSYNC", "AS"),
    ("insert", "Reference", "RE"),
    ("insert", XREF_LAUNCHER, "XR"),
    ("insert", "XATTACH", "XA"),
    ("insert", "UNDERLAY_ATTACH", "UA"),
    ("insert", "CLIP", "CL"),
    ("insert", "ADJUST", "AJ"),
    ("insert", "UNDERLAYLAYERS", "UN"),
    ("insert", "FRAMES_DROPDOWN", "FR"),
    ("insert", "UOSNAP_DROPDOWN", "SN"),
    ("reference_extension", "REFEDIT", "ER"),
    ("reference_extension", XREF_FADE, "XF"),
    // Annotate.
    ("annotate", "Text", "TE"),
    ("annotate", "ANNOTATE_TEXT", "MT"),
    ("annotate", "TEXT_STYLE_COMBO", "TY"),
    ("annotate", "Dimensions", "DM"),
    ("annotate", "ANNOTATE_DIM", "DN"),
    ("annotate", "DIM_STYLE_COMBO", "DY"),
    ("annotate", "QDIM", "QD"),
    ("annotate", "DIMCONTINUE", "CO"),
    ("annotate", "TOLERANCE", "TO"),
    ("annotate", "DIMEDIT", "DE"),
    ("annotate", "DIMBREAK", "BR"),
    ("annotate", "DIMSPACE", "SP"),
    ("annotate", "DIMJOGLINE", "DJ"),
    ("annotate", "Centerlines", "CE"),
    ("annotate", "CENTERMARK", "CM"),
    ("annotate", "CENTERLINE", "CL"),
    ("annotate", "ANNOTATE_LEADER", "ML"),
    ("annotate", "MLEADER_STYLE_COMBO", "MY"),
    ("annotate", "MLEADERADD", "MAD"),
    ("annotate", "MLEADERREMOVE", "MV"),
    ("annotate", "MLEADERALIGN", "MAL"),
    ("annotate", "MLEADERCOLLECT", "MC"),
    ("annotate", "TABLE", "TB"),
    ("annotate", "TABLE_STYLE_COMBO", "TL"),
    ("annotate", "DATALINK", "DL"),
    ("annotate", "WIPEOUT", "WI"),
    ("annotate", "REVCLOUD", "AC"),
    ("annotate", "SCALELISTEDIT", "SL"),
    ("annotate", "OBJECTSCALE", "AS"),
    ("annotate", "ANNORESET", "SS"),
    // View.
    ("view", "Viewport Tools", "VT"),
    ("view", "UCSICON", "UI"),
    ("view", "NAVVCUBE", "VC"),
    ("view", "PAN", "PA"),
    ("view", "3DORBIT", "OR"),
    ("view", "Model Viewports", "VP"),
    ("view", "VPORTS_NAMED", "NP"),
    ("view", "VPJOIN", "JV"),
    ("view", "VPORTS_RESTORE", "RV"),
    ("view", "Visual Style", "VY"),
    ("view", "VISUAL_STYLE", "VS"),
    ("view", "TOOLPALETTES", "TP"),
    ("view", "PROPERTIES", "PR"),
    ("view", "COUNTLIST", "CT"),
    ("view", "SHEETSET", "SS"),
    ("view", "FILETAB", "FT"),
    ("view", "LAYOUTTAB", "LT"),
    ("view", "HORIZONTAL", "TH"),
    ("view", "VERTICAL", "TV"),
    ("view", "CASCADE", "CA"),
    // Manage.
    ("manage", "CUI", "CU"),
    ("manage", "TOOLPALETTES", "TP"),
    ("manage", "CUIIMPORT", "IM"),
    ("manage", "CUIEXPORT", "EX"),
    ("manage", "ALIASEDIT_DROPDOWN", "EA"),
    ("ALIASEDIT_DROPDOWN", "ALIASEDIT", "EA"),
    ("manage", "FINDNONPURGEABLE", "FN"),
    ("manage", "PURGE", "PU"),
    ("manage", "OVERKILL", "OK"),
    ("manage", "AUDIT", "AU"),
    // Block editor.
    ("block_editor", "Open and Save", "BO"),
    ("block_editor", "BEDIT", "BE"),
    ("block_editor", "BSAVE", "BS"),
    ("block_editor", "ATTDEF", "AD"),
    ("block_editor", "AUTOCONSTRAIN", "AC"),
    ("block_editor", "CCONSTRAINT", "CI"),
    ("block_editor", "PCONSTRAINT", "PA"),
    ("block_editor", "TCONSTRAINT", "TA"),
    // The reference gives Concentric the same code; Collinear comes first.
    ("block_editor", "LCONSTRAINT", "CO"),
    ("block_editor", "QCONSTRAINT", "PE"),
    ("block_editor", "GCSMOOTH", "SM"),
    ("block_editor", "GCHORIZONTAL", "HO"),
    ("block_editor", "SYCONSTRAINT", "SY"),
    ("block_editor", "FXCONSTRAINT", "FI"),
    ("block_editor", "VCONSTRAINT", "VE"),
    ("block_editor", "ECONSTRAINT", "EQ"),
    ("block_editor", "GCVISIBILITY", "SH"),
    ("block_editor", "GCSHOWALL", "SA"),
    ("block_editor", "GCHIDEALL", "HA"),
    ("block_editor", "DC_LINEAR_MENU", "DC"),
    ("block_editor", "DCALIGNED", "AL"),
    ("block_editor", "DCRADIUS", "RA"),
    ("block_editor", "DCDIAMETER", "DI"),
    ("block_editor", "DCANGULAR", "AN"),
    ("block_editor", "DCCONVERT", "BV"),
    ("block_editor", "DELCONSTRAINT", "DE"),
    ("block_editor", "PARAMETERS", "PM"),
];

fn code(scope: &str, target: &str) -> Option<&'static str> {
    CODES
        .iter()
        .find(|(s, t, _)| *s == scope && *t == target)
        .map(|(_, _, code)| *code)
}

fn tool_tip(scope: &str, tool: &ToolDef) -> Tip {
    Tip {
        anchor: tool.id.to_string(),
        target: tool.id,
        code: code(scope, tool.id).map(str::to_string),
        action: Action::Run(Message::RibbonToolClick {
            tool_id: tool.id.to_string(),
            event: tool.event.clone(),
        }),
    }
}

fn dropdown_tip(scope: &str, id: &'static str) -> Tip {
    Tip {
        anchor: id.to_string(),
        target: id,
        code: code(scope, id).map(str::to_string),
        action: Action::Open(Message::ToggleRibbonDropdown(id.to_string())),
    }
}

impl Ribbon {
    pub fn keytips_active(&self) -> bool {
        self.keytips.is_some()
    }

    pub fn show_keytips(&mut self, refocus: bool) {
        self.close_dropdown();
        self.collapsed_open = None;
        self.keytips = Some(State {
            top: true,
            path: Vec::new(),
            typed: String::new(),
            refocus,
        });
        ACTIVE.store(true, Ordering::Relaxed);
        // Bounds reported before now belong to an earlier frame.
        crate::ui::wrap_bar::end_frame();
    }

    /// Close the tips; true when the command line should get the focus back.
    pub fn hide_keytips(&mut self) -> bool {
        ACTIVE.store(false, Ordering::Relaxed);
        self.keytips.take().is_some_and(|state| state.refocus)
    }

    /// Esc: drop a partial code, close the open dropdown or flyout, or go
    /// back to the tab codes. False when the tips should close.
    pub fn keytip_escape(&mut self) -> bool {
        let dropdown_open = self.open_dropdown.is_some();
        let flyout_open = self.collapsed_open.is_some();
        let Some(state) = self.keytips.as_mut() else {
            return false;
        };
        if !state.typed.is_empty() {
            state.typed.clear();
            return true;
        }
        if state.top {
            return false;
        }
        state.path.pop();
        if dropdown_open {
            // A submenu goes back to its slide-out.
            if !self.escape_extension() {
                self.close_dropdown();
            }
        } else if flyout_open {
            self.collapsed_open = None;
        } else {
            state.top = true;
            state.path.clear();
        }
        true
    }

    /// One typed character; `None` confirms a code that is also the start of
    /// a longer one (Enter).
    pub fn keytip_type(&mut self, ch: Option<&str>) -> KeyTipStep {
        let tips = self.keytip_list();
        let Some(state) = self.keytips.as_mut() else {
            return KeyTipStep::Ignore;
        };
        let codes = || tips.iter().filter_map(|tip| tip.code.as_deref());
        let exact = |typed: &str| tips.iter().find(|tip| tip.code.as_deref() == Some(typed));
        let longer = |typed: &str| codes().any(|code| code.len() > typed.len() && code.starts_with(typed));
        let typed = ch.map(|ch| format!("{}{ch}", state.typed));
        let chosen = match typed {
            Some(typed) if codes().any(|code| code.starts_with(&typed)) => {
                if longer(&typed) || exact(&typed).is_none() {
                    state.typed = typed;
                    return KeyTipStep::Stay;
                }
                typed
            }
            // A key that extends no code (or Enter) settles a code that is
            // also the start of a longer one ("3" beside "3D").
            _ if exact(&state.typed).is_some() => std::mem::take(&mut state.typed),
            _ => return KeyTipStep::Ignore,
        };
        let Some(tip) = exact(&chosen) else {
            return KeyTipStep::Ignore;
        };
        state.typed.clear();
        state.path.push(chosen);
        match tip.action.clone() {
            Action::Tab(index) => {
                state.top = false;
                self.select(index);
                KeyTipStep::Stay
            }
            Action::Open(message) => KeyTipStep::Open(message),
            Action::Run(message) => KeyTipStep::Run(message),
        }
    }

    /// The codes of the current level, with the widgets they sit on.
    pub fn keytip_list(&self) -> Vec<Tip> {
        let Some(state) = &self.keytips else {
            return Vec::new();
        };
        if state.top {
            return self.tab_tips();
        }
        if let Some(open) = self.open_dropdown.as_deref() {
            return self.item_tips(self.active, open);
        }
        self.module_tips(self.active)
    }

    fn tab_tips(&self) -> Vec<Tip> {
        let tabs = self.modules.iter().enumerate().filter(|(_, module)| {
            module.id() != "layout"
                && (module.id() != "block_editor" || self.block_editing.is_some())
        });
        let mut tips: Vec<Tip> = tabs
            .map(|(index, module)| Tip {
                anchor: format!("tab:{}", module.id()),
                target: module.id(),
                code: tab_code(module.id()).map(str::to_string),
                action: Action::Tab(index),
            })
            .collect();
        tips.extend(QUICK_ACCESS.iter().enumerate().map(|(index, &id)| Tip {
            anchor: quick_access_anchor(id),
            target: id,
            code: Some((index + 1).to_string()),
            action: Action::Run(match id {
                super::widgets::UNDO_HISTORY_ID => Message::Undo,
                super::widgets::REDO_HISTORY_ID => Message::Redo,
                command => Message::Command(command.to_string()),
            }),
        }));
        tips
    }

    /// Every button, dropdown, slide-out and collapsed panel of tab `index`.
    fn module_tips(&self, index: usize) -> Vec<Tip> {
        let Some(module) = self.modules.get(index) else {
            return Vec::new();
        };
        let scope = module.id();
        let mut tips = Vec::new();
        for group in module.ribbon_groups() {
            for item in &group.tools {
                match item {
                    RibbonItem::Tool(tool)
                    | RibbonItem::LabeledTool(tool)
                    | RibbonItem::LargeTool(tool) => tips.push(tool_tip(scope, tool)),
                    RibbonItem::Dropdown { id, .. }
                    | RibbonItem::LabeledDropdown { id, .. }
                    | RibbonItem::LargeDropdown { id, .. } => tips.push(dropdown_tip(scope, id)),
                    RibbonItem::ToolGrid { columns } => {
                        tips.extend(columns.iter().flatten().map(|tool| tool_tip(scope, tool)))
                    }
                    RibbonItem::LayerComboGroup { row2, row3 } => {
                        tips.push(dropdown_tip(scope, super::widgets::LAYER_COMBO_ID));
                        tips.extend(row2.iter().chain(row3).map(|tool| tool_tip(scope, tool)));
                    }
                    RibbonItem::PropertiesGroup { match_prop } => {
                        tips.push(tool_tip(scope, match_prop));
                        for id in [
                            super::widgets::PROP_COLOR_ID,
                            super::widgets::PROP_LINETYPE_ID,
                            super::widgets::PROP_LW_ID,
                        ] {
                            tips.push(dropdown_tip(scope, id));
                        }
                    }
                    RibbonItem::StyleComboGroup { combo_id, rows, .. } => {
                        tips.push(dropdown_tip(scope, combo_id));
                        tips.extend(rows.iter().flatten().map(|tool| tool_tip(scope, tool)));
                    }
                }
            }
            // The panel code opens the collapsed panel's flyout, or else the
            // panel's slide-out. Both carry it; only the drawn one shows, and
            // the form the panel had in the last frame comes first, so the
            // code runs that one.
            let ids: Vec<&'static str> = group.tools.iter().filter_map(item_id).collect();
            let collapsed = collapsed_anchor(group.title);
            let was_collapsed = crate::ui::wrap_bar::recent_bounds(&collapsed).is_some();
            let flyout = Tip {
                anchor: collapsed,
                target: group.title,
                code: code(scope, group.title).map(str::to_string),
                action: Action::Open(Message::ToggleRibbonPanel(group.title.to_string())),
            };
            let Some(id) = draw_panel::slide_out_id(group.title, &ids) else {
                tips.push(flyout);
                continue;
            };
            let slide_out = Tip {
                anchor: id.to_string(),
                target: group.title,
                code: code(scope, group.title).map(str::to_string),
                action: Action::Open(Message::ToggleRibbonDropdown(id.to_string())),
            };
            // In the open flyout the code reaches the slide-out.
            if was_collapsed && self.collapsed_open.as_deref() != Some(group.title) {
                tips.extend([flyout, slide_out]);
            } else {
                tips.extend([slide_out, flyout]);
            }
            if id == draw_panel::REFERENCE_PANEL_ID {
                tips.push(Tip {
                    anchor: XREF_LAUNCHER.to_string(),
                    target: XREF_LAUNCHER,
                    code: code(scope, XREF_LAUNCHER).map(str::to_string),
                    action: Action::Run(Message::ToggleXrefManager),
                });
            }
        }
        tips
    }

    /// The items of the open dropdown, slide-out or submenu `open` of tab
    /// `index`. Items the reference gives no code are numbered.
    fn item_tips(&self, index: usize, open: &str) -> Vec<Tip> {
        let mut items = draw_panel::keytip_items(open);
        if items.is_empty() {
            let groups = self.modules.get(index).map(|m| m.ribbon_groups()).unwrap_or(&[]);
            for item in groups.iter().flat_map(|group| &group.tools) {
                if let RibbonItem::Dropdown { id, items: list, .. }
                | RibbonItem::LabeledDropdown { id, items: list, .. }
                | RibbonItem::LargeDropdown { id, items: list, .. } = item
                {
                    if *id == open {
                        items = list
                            .iter()
                            .filter(|(cmd, _, _)| !super::widgets::is_disabled_item(cmd))
                            .map(|(cmd, _, _)| (*id, *cmd, false))
                            .collect();
                        break;
                    }
                }
            }
        }
        let mut tips: Vec<Tip> = items
            .into_iter()
            .enumerate()
            .map(|(position, (dropdown_id, cmd, submenu))| Tip {
                anchor: item_anchor(dropdown_id, cmd),
                target: cmd,
                code: Some(code(open, cmd).map_or_else(|| (position + 1).to_string(), str::to_string)),
                action: if submenu {
                    Action::Open(Message::ToggleRibbonDropdown(cmd.to_string()))
                } else {
                    Action::Run(Message::DropdownSelectItem { dropdown_id, cmd })
                },
            })
            .collect();
        if open == draw_panel::REFERENCE_PANEL_ID {
            tips.push(Tip {
                anchor: item_anchor(open, XREF_FADE),
                target: XREF_FADE,
                code: code(open, XREF_FADE).map(str::to_string),
                action: Action::Run(Message::XrefFadeToggle),
            });
        }
        tips
    }

    /// The state for automation: level, path, and the codes with whether
    /// their widget showed in the last frame. `all` adds every tab's buttons
    /// and their dropdown items, coded or not.
    pub fn keytips_json(&self, all: bool) -> serde_json::Value {
        use serde_json::json;
        let shown = SHOWN.with(|shown| shown.borrow().clone());
        let entry = |tip: &Tip| {
            json!({
                "code": tip.code,
                "target": tip.target,
                "anchor": tip.anchor,
                "shown": shown.contains(&tip.anchor),
            })
        };
        let mut out = match &self.keytips {
            Some(state) => json!({
                "active": true,
                "level": if state.top { "tabs" } else if self.open_dropdown.is_some() { "items" } else { "tab" },
                "tab": self.modules.get(self.active).map(|m| m.id()),
                "open": self.open_dropdown,
                "path": state.path,
                "typed": state.typed,
                "tips": self.keytip_list().iter().filter(|tip| tip.code.is_some()).map(entry).collect::<Vec<_>>(),
            }),
            None => json!({"active": false}),
        };
        if all {
            let tabs: Vec<_> = (0..self.modules.len())
                .map(|index| {
                    let buttons: Vec<_> = self
                        .module_tips(index)
                        .iter()
                        .map(|tip| {
                            let mut value = entry(tip);
                            let items = self.item_tips(index, &tip.anchor);
                            if !items.is_empty() {
                                value["items"] = items.iter().map(entry).collect();
                            }
                            value
                        })
                        .collect();
                    json!({"tab": self.modules[index].id(), "code": tab_code(self.modules[index].id()), "buttons": buttons})
                })
                .collect();
            out["all"] = json!(tabs);
        }
        out
    }

    /// The hint bar layer and the codes layer.
    pub fn keytip_layers(&self) -> Option<(Element<'_, Message>, Element<'_, Message>)> {
        let state = self.keytips.as_ref()?;
        let tips: Vec<(String, String)> = self
            .keytip_list()
            .into_iter()
            .filter_map(|tip| {
                let code = tip.code?;
                code.starts_with(&state.typed).then_some((tip.anchor, code))
            })
            .collect();
        let mut path = state.path.join(" › ");
        if !state.top {
            path.push_str(" › ");
        }
        path.push_str(&state.typed);
        if path.is_empty() {
            path.push('—');
        }
        let hint = if state.top {
            crate::tr!("keytips", "hint-tabs")
        } else if self.open_dropdown.is_some() || self.collapsed_open.is_some() {
            crate::tr!("keytips", "hint-items")
        } else {
            let module = &self.modules[self.active];
            crate::tr!(
                "keytips",
                "hint-tab",
                tab = crate::i18n::ribbon_module_title(module.id(), module.title())
            )
        };
        let bar = KeyTipLayer {
            tips: Vec::new(),
            typed: 0,
            bar: Some([
                crate::tr!("keytips", "title"),
                crate::tr!("keytips", "typed", path = path),
                hint,
            ]),
        };
        let codes = KeyTipLayer {
            tips,
            typed: state.typed.chars().count(),
            bar: None,
        };
        Some((Element::new(bar), Element::new(codes)))
    }
}

/// Fills the window without taking input. With `bar` it draws the hint bar
/// (beneath open dropdowns); otherwise it draws the codes in an overlay, so
/// they sit above flyouts and dropdowns, after their widgets reported where
/// they are.
struct KeyTipLayer {
    tips: Vec<(String, String)>,
    /// Characters of a partial code, shown dimmed on matching tips.
    typed: usize,
    bar: Option<[String; 3]>,
}

impl Widget<Message, Theme, Renderer> for KeyTipLayer {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.max())
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        if let Some(parts) = &self.bar {
            draw_bar(renderer, theme, parts);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        _tree: &'b mut widget::Tree,
        _layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        _translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.bar
            .is_none()
            .then(|| overlay::Element::new(Box::new(KeyTipOverlay { layer: self })))
    }
}

struct KeyTipOverlay<'b> {
    layer: &'b KeyTipLayer,
}

const PILL_H: f32 = 16.0;
const PILL_TEXT: f32 = 11.0;
const BAR_H: f32 = 24.0;
const BAR_TEXT: f32 = 12.0;

fn bold(renderer: &Renderer) -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Semibold,
        ..renderer.default_font()
    }
}

fn text_width(content: &str, size: f32, font: iced::Font) -> f32 {
    <Renderer as advanced_text::Renderer>::Paragraph::with_text(advanced_text::Text {
        content,
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: advanced_text::LineHeight::default(),
        font,
        align_x: advanced_text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: advanced_text::Shaping::Advanced,
        wrapping: advanced_text::Wrapping::None,
        ellipsis: advanced_text::Ellipsis::None,
        hint_factor: None,
    })
    .min_bounds()
    .width
}

fn draw_text(
    renderer: &mut Renderer,
    content: &str,
    at: Point,
    size: f32,
    font: iced::Font,
    color: Color,
    centered: bool,
    clip: Rectangle,
) {
    renderer.fill_text(
        advanced_text::Text {
            content: content.to_string(),
            bounds: Size::INFINITE,
            size: Pixels(size),
            line_height: advanced_text::LineHeight::default(),
            font,
            align_x: if centered {
                advanced_text::Alignment::Center
            } else {
                advanced_text::Alignment::Left
            },
            align_y: iced::alignment::Vertical::Center,
            shaping: advanced_text::Shaping::Advanced,
            wrapping: advanced_text::Wrapping::None,
            ellipsis: advanced_text::Ellipsis::None,
            hint_factor: None,
        },
        at,
        color,
        clip,
    );
}

/// Pill and text colours: a dark accent-edged pill on dark themes, a light
/// one on light themes.
fn colors(theme: &Theme) -> (Color, Color, Color) {
    let palette = theme.palette();
    let accent = palette.primary.base.color;
    if palette.is_dark {
        (
            Color::from_rgb8(0x0f, 0x2a, 0x36),
            accent,
            Color::from_rgb8(0x7f, 0xe0, 0xff),
        )
    } else {
        (
            Color::from_rgb8(0xe3, 0xf5, 0xfd),
            accent,
            Color::from_rgb8(0x00, 0x4a, 0x6b),
        )
    }
}

impl overlay::Overlay<Message, Theme, Renderer> for KeyTipOverlay<'_> {
    fn layout(&mut self, _renderer: &Renderer, bounds: Size) -> layout::Node {
        layout::Node::new(bounds)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        let window = layout.bounds();
        let (fill, edge, ink) = colors(theme);
        let font = bold(renderer);
        let mut shown = Vec::new();
        renderer.with_layer(window, |renderer| {
            for (anchor, code) in &self.layer.tips {
                let Some(b) = fresh_bounds(anchor) else {
                    continue;
                };
                let width = text_width(code, PILL_TEXT, font) + 10.0;
                let top_row = anchor.starts_with("tab:")
                    || anchor.starts_with("qa:")
                    || anchor == super::widgets::UNDO_HISTORY_ID
                    || anchor == super::widgets::REDO_HISTORY_ID;
                let center = if top_row {
                    // Tabs and quick-access buttons: on the lower edge, so the
                    // label stays readable.
                    Point::new(b.center_x(), b.y + b.height - 2.0)
                } else if anchor.contains('/') && b.width >= 60.0 {
                    // Menu rows: at the right end, clear of the label.
                    Point::new(b.x + b.width - width / 2.0 - 4.0, b.center_y())
                } else if anchor.contains('/') {
                    // Slide-out icons: on the lower edge.
                    Point::new(b.center_x(), b.y + b.height - 2.0)
                } else if b.height >= 40.0 {
                    // Large buttons: over the label.
                    Point::new(b.center_x(), b.y + b.height - PILL_H / 2.0 - 2.0)
                } else {
                    // Small buttons: at the lower left, over the icon.
                    Point::new(b.x + width / 2.0, b.y + b.height - PILL_H / 2.0 + 3.0)
                };
                let x = (center.x - width / 2.0).clamp(window.x, (window.width - width).max(window.x));
                let y = (center.y - PILL_H / 2.0).clamp(window.y, (window.height - PILL_H).max(window.y));
                let pill = Rectangle::new(Point::new(x, y), Size::new(width, PILL_H));
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: pill,
                        border: Border {
                            color: edge,
                            width: 1.0,
                            radius: (PILL_H / 2.0).into(),
                        },
                        shadow: Shadow {
                            color: Color::BLACK.scale_alpha(0.6),
                            offset: Vector::new(0.0, 1.0),
                            blur_radius: 4.0,
                        },
                        snap: true,
                    },
                    Background::Color(fill),
                );
                // The part already typed is dimmed.
                let split = code
                    .char_indices()
                    .nth(self.layer.typed)
                    .map_or(code.len(), |(at, _)| at);
                let (done, rest) = code.split_at(split);
                let done_w = text_width(done, PILL_TEXT, font);
                let rest_w = text_width(rest, PILL_TEXT, font);
                let left = pill.center_x() - (done_w + rest_w) / 2.0;
                let clip = Rectangle {
                    width: pill.width + 2.0,
                    ..pill
                };
                if !done.is_empty() {
                    draw_text(renderer, done, Point::new(left, pill.center_y() - 0.5), PILL_TEXT, font, ink.scale_alpha(0.45), false, clip);
                }
                draw_text(renderer, rest, Point::new(left + done_w, pill.center_y() - 0.5), PILL_TEXT, font, ink, false, clip);
                shown.push(anchor.clone());
            }
        });
        SHOWN.with(|cell| *cell.borrow_mut() = shown);
        crate::ui::wrap_bar::end_frame();
    }
}

/// The hint bar under the ribbon: title, the typed path and what to do next.
fn draw_bar(renderer: &mut Renderer, theme: &Theme, parts: &[String; 3]) {
    let Some(ribbon) = fresh_bounds(RIBBON_AREA) else {
        return;
    };
    let (fill, edge, ink) = colors(theme);
    let font = bold(renderer);
    let bar = Rectangle::new(
        Point::new(ribbon.x, ribbon.y + ribbon.height),
        Size::new(ribbon.width, BAR_H),
    );
    renderer.fill_quad(
        renderer::Quad {
            bounds: bar,
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        },
        Background::Color(fill),
    );
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle::new(bar.position(), Size::new(bar.width, 1.0)),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        },
        Background::Color(edge),
    );
    let mut x = bar.x + 12.0;
    let y = bar.center_y();
    let regular = renderer.default_font();
    for (index, part) in parts.iter().enumerate() {
        let (part_font, color) = match index {
            0 => (font, ink),
            1 => (regular, ink),
            _ => (regular, ink.scale_alpha(0.6)),
        };
        draw_text(renderer, part, Point::new(x, y), BAR_TEXT, part_font, color, false, bar);
        x += text_width(part, BAR_TEXT, part_font) + 18.0;
    }
}

/// Anchor for the whole ribbon (the hint bar sits under it).
pub(super) fn ribbon_area<'a>(el: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    anchor(RIBBON_AREA, el)
}
