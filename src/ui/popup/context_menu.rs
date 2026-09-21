//! Viewport right-click context menu — the *model*.
//!
//! `build_context_menu` is a pure function from the app's current state
//! (`MenuContext`) to the rows the menu shows (`ContextMenu`). It knows nothing
//! about iced: the view renders the rows, and the keyboard handler in `update`
//! walks the same rows to resolve Up/Down/Enter/mnemonic picks, so what the
//! user sees and what a keypress selects can never diverge.
//!
//! The layout covers the shortcut menus of commercial solutions, which the user base already
//! knows: while a command runs the menu offers Enter / Cancel / the command's
//! own keyword options (with the keyword shown as a hint, so the menu also
//! teaches the typed shortcut); when idle it offers Repeat / Recent Input /
//! Clipboard / Undo-Redo / Isolate / Pan-Zoom / selection tools; during a grip
//! edit it offers the grip modes.

use crate::command::CmdOption;
use crate::scene::parametric_constraints::ConstraintId;
use crate::scene::pick::grip::GripEditMode;
use crate::snap::SnapType;
use crate::t;

/// Fixed row height used by the renderer, so `ContextMenu::default_row_y`
/// (which anchors the default row under the cursor) agrees with the layout.
pub const MENU_ROW_H: f32 = 22.0;
/// Height of a separator row (1 px line plus its vertical padding).
pub const MENU_SEP_H: f32 = 7.0;
/// Vertical padding inside the menu panel above the first row.
pub const MENU_PAD_TOP: f32 = 4.0;
/// Column width without keyword hints (the width the menu always had).
pub const MENU_WIDTH: f32 = 200.0;
/// Column width when any row carries a right-aligned keyword hint.
pub const MENU_WIDTH_WITH_HINTS: f32 = 236.0;
/// How many recent entries the Recent Input / Repeat lists show.
pub const RECENT_LIMIT: usize = 10;

/// Identifies an inline-expanding (accordion) submenu. Only one is open at a
/// time; the open one is tracked in `SelectionState::context_menu_ui`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmenuId {
    RecentInput,
    Clipboard,
    DrawOrder,
    Isolate,
    SnapOverrides,
}

/// Glyph drawn in a row's icon gutter (commercial solutions show one for object snaps
/// and the navigation rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuIcon {
    Snap(SnapType),
    Mtp,
    Pan,
    Zoom,
}

/// What a grip-menu row does (the grip-mode shortcut menu of commercial solutions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GripMenuCmd {
    Stretch,
    Move,
    Rotate,
    Scale,
    Mirror,
    BasePoint,
    CopyToggle,
    Undo,
    Exit,
}

/// Everything a menu row can do. Translated into the app's existing messages
/// by `OpenCADStudio::on_context_menu_pick`, so a row does exactly what the
/// equivalent typed input / shortcut does.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuAction {
    /// Enter — finish / accept the current step (`CommandFinalize`).
    Enter,
    /// Escape — cancel the running command (`CommandEscape`).
    Cancel,
    /// Feed a command option keyword (`CommandOptionPick`). Empty = Enter.
    Option(String),
    /// Re-feed a recent typed token (point, distance, keyword) to the
    /// running command.
    FeedInput(String),
    /// Run a command line string (`Message::Command`).
    Command(String),
    /// One-shot "Mid Between 2 Points" snap override.
    Mtp,
    /// One-shot object snap override for the next pick.
    SnapOverride(SnapType),
    /// Next pick ignores object snaps.
    SnapOverrideNone,
    DeleteSelected,
    SelectSimilar,
    InvertSelection,
    DeselectAll,
    QuickSelect,
    Properties,
    Options,
    Undo,
    Redo,
    DrawOrderPickRef(bool),
    ConstraintDelete(ConstraintId),
    Grip(GripMenuCmd),
    /// Expand / collapse an accordion submenu (never "picked" as a command).
    ToggleSubmenu(SubmenuId),
}

/// One selectable row.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub label: String,
    /// Right-aligned keyword hint, e.g. `"C"` for Close, `"⏎"` for the Enter
    /// row. Teaches the typed shortcut without cluttering the label.
    pub hint: Option<String>,
    /// Uppercase letter that picks this row from the keyboard while the menu
    /// is open. Derived from the option keyword's first character.
    pub mnemonic: Option<char>,
    pub action: MenuAction,
    /// The row Enter picks when nothing is highlighted; rendered bold and
    /// anchored under the cursor when the menu opens.
    pub default: bool,
    pub enabled: bool,
    /// Rendered with a leading check mark (toggle / current-state rows).
    pub checked: bool,
    /// Glyph in the icon gutter.
    pub icon: Option<MenuIcon>,
}

impl MenuItem {
    fn new(label: impl Into<String>, action: MenuAction) -> Self {
        Self {
            label: label.into(),
            hint: None,
            mnemonic: None,
            action,
            default: false,
            enabled: true,
            checked: false,
            icon: None,
        }
    }

    fn icon(mut self, icon: MenuIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    fn mnemonic(mut self, ch: Option<char>) -> Self {
        self.mnemonic = ch;
        self
    }

    fn default(mut self) -> Self {
        self.default = true;
        self
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuRow {
    Item(MenuItem),
    Separator,
    Submenu {
        id: SubmenuId,
        label: String,
        items: Vec<MenuItem>,
        open: bool,
    },
}

/// A keyboard-navigable row in display order: either an item or a submenu
/// header. Separators and the children of collapsed submenus are skipped.
#[derive(Clone, Debug, PartialEq)]
pub struct Selectable {
    pub action: MenuAction,
    pub mnemonic: Option<char>,
    pub enabled: bool,
    /// `Some(id)` for a submenu header row.
    pub header: Option<SubmenuId>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextMenu {
    pub rows: Vec<MenuRow>,
    pub width: f32,
}

impl ContextMenu {
    /// Keyboard-navigable rows in display order. The index into this list is
    /// the highlight index kept in `ContextMenuUi::highlighted`.
    pub fn selectable(&self) -> Vec<Selectable> {
        let mut out = Vec::new();
        for row in &self.rows {
            match row {
                MenuRow::Item(item) => out.push(Selectable {
                    action: item.action.clone(),
                    mnemonic: item.mnemonic,
                    enabled: item.enabled,
                    header: None,
                }),
                MenuRow::Separator => {}
                MenuRow::Submenu { id, items, open, .. } => {
                    out.push(Selectable {
                        action: MenuAction::ToggleSubmenu(*id),
                        mnemonic: None,
                        enabled: !items.is_empty(),
                        header: Some(*id),
                    });
                    if *open {
                        for item in items {
                            out.push(Selectable {
                                action: item.action.clone(),
                                mnemonic: item.mnemonic,
                                enabled: item.enabled,
                                header: None,
                            });
                        }
                    }
                }
            }
        }
        out
    }

    /// Index (into `selectable()`) of the default row, else the first row.
    pub fn default_index(&self) -> usize {
        let mut idx = 0;
        for row in &self.rows {
            match row {
                MenuRow::Item(item) => {
                    if item.default {
                        return idx;
                    }
                    idx += 1;
                }
                MenuRow::Separator => {}
                MenuRow::Submenu { items, open, .. } => {
                    idx += 1;
                    if *open {
                        for item in items {
                            if item.default {
                                return idx;
                            }
                            idx += 1;
                        }
                    }
                }
            }
        }
        0
    }

    /// The enabled row whose mnemonic is `ch`, searching forward from (and
    /// excluding) `from` and wrapping, so repeated presses cycle through rows
    /// sharing a letter (PLINE's CEnter / CLose both answer to `C`).
    pub fn find_mnemonic(&self, ch: char, from: Option<usize>) -> Option<usize> {
        let ch = ch.to_ascii_uppercase();
        let rows = self.selectable();
        if rows.is_empty() {
            return None;
        }
        let start = from.map_or(0, |f| (f + 1) % rows.len());
        (0..rows.len())
            .map(|k| (start + k) % rows.len())
            .find(|&i| rows[i].enabled && rows[i].mnemonic == Some(ch))
    }

    /// Vertical offset from the panel's top edge to the top of the default
    /// row, using the renderer's fixed row metrics.
    pub fn default_row_y(&self) -> f32 {
        let mut y = MENU_PAD_TOP;
        for row in &self.rows {
            match row {
                MenuRow::Item(item) => {
                    if item.default {
                        return y;
                    }
                    y += MENU_ROW_H;
                }
                MenuRow::Separator => y += MENU_SEP_H,
                MenuRow::Submenu { items, open, .. } => {
                    y += MENU_ROW_H;
                    if *open {
                        for item in items {
                            if item.default {
                                return y;
                            }
                            y += MENU_ROW_H;
                        }
                    }
                }
            }
        }
        MENU_PAD_TOP
    }

    /// Total panel height (rows plus the panel's vertical padding).
    pub fn height(&self) -> f32 {
        let mut h = MENU_PAD_TOP * 2.0;
        for row in &self.rows {
            match row {
                MenuRow::Item(_) => h += MENU_ROW_H,
                MenuRow::Separator => h += MENU_SEP_H,
                MenuRow::Submenu { items, open, .. } => {
                    h += MENU_ROW_H;
                    if *open {
                        h += items.len() as f32 * MENU_ROW_H;
                    }
                }
            }
        }
        h
    }

    fn has_hints(&self) -> bool {
        self.rows.iter().any(|row| match row {
            MenuRow::Item(item) => item.hint.is_some(),
            MenuRow::Separator => false,
            MenuRow::Submenu { items, .. } => items.iter().any(|item| item.hint.is_some()),
        })
    }
}

/// State of a grip edit the menu was opened over.
#[derive(Clone, Debug, PartialEq)]
pub struct GripMenuContext {
    pub mode: GripEditMode,
    pub copy_on: bool,
    /// The grip has been dragged away from its origin (Undo snaps it back).
    pub moved: bool,
}

/// Inputs the builder needs, gathered by `OpenCADStudio::context_menu_context`.
#[derive(Clone, Debug)]
pub enum MenuContext {
    /// A command is running: its current step's options.
    Command {
        options: Vec<CmdOption>,
        /// The step takes a point pick (the M2P override applies).
        has_point_step: bool,
        /// Recently typed tokens (newest first) for the Recent Input submenu.
        recent_inputs: Vec<String>,
    },
    /// Nothing running.
    Idle {
        has_selection: bool,
        selected_constraint: Option<ConstraintId>,
        isolation_active: bool,
        /// Recently run commands, newest first.
        recent_cmds: Vec<String>,
        clipboard_nonempty: bool,
        undo_label: Option<String>,
        redo_label: Option<String>,
        props_open: bool,
    },
    /// A grip is hot / being dragged.
    Grip(GripMenuContext),
}

/// Build the menu rows for `ctx`, with `open_submenu` expanded.
pub fn build_context_menu(ctx: &MenuContext, open_submenu: Option<SubmenuId>) -> ContextMenu {
    let rows = match ctx {
        MenuContext::Command {
            options,
            has_point_step,
            recent_inputs,
        } => command_rows(options, *has_point_step, recent_inputs, open_submenu),
        MenuContext::Idle {
            has_selection,
            selected_constraint,
            isolation_active,
            recent_cmds,
            clipboard_nonempty,
            undo_label,
            redo_label,
            props_open,
        } => idle_rows(
            *has_selection,
            *selected_constraint,
            *isolation_active,
            recent_cmds,
            *clipboard_nonempty,
            undo_label.as_deref(),
            redo_label.as_deref(),
            *props_open,
            open_submenu,
        ),
        MenuContext::Grip(grip) => grip_rows(grip),
    };
    let mut menu = ContextMenu { rows, width: MENU_WIDTH };
    if menu.has_hints() {
        menu.width = MENU_WIDTH_WITH_HINTS;
    }
    menu
}

/// Keyboard letter for an option keyword: its first ASCII alphanumeric.
fn mnemonic_for(keyword: &str) -> Option<char> {
    keyword
        .chars()
        .find(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
}

fn command_rows(
    options: &[CmdOption],
    has_point_step: bool,
    recent_inputs: &[String],
    open_submenu: Option<SubmenuId>,
) -> Vec<MenuRow> {
    let mut rows = Vec::new();
    // Ordered by how often a drafter reaches for each row: Enter / Cancel
    // under the pointer, the command's own keywords right below them (the
    // choices that change from step to step), then the recall and snap
    // helpers, and the transparent navigation commands last.
    //
    // A command may name its own finish action (PLINE "Done", OFFSET's
    // "<0.5000>" default). That row *is* Enter: it is not listed twice, and
    // a default value is shown as the Enter row's hint.
    let finish = options.iter().find(|o| o.keyword.is_empty());
    let enter_hint = match finish {
        Some(o) if o.label.starts_with('<') => o.label.clone(),
        _ => "⏎".to_string(),
    };
    rows.push(MenuRow::Item(
        MenuItem::new(t!("Enter").into_owned(), MenuAction::Enter).hint(enter_hint).default(),
    ));
    rows.push(MenuRow::Item(
        MenuItem::new(t!("Cancel").into_owned(), MenuAction::Cancel).hint("Esc"),
    ));

    let keyword_opts: Vec<&CmdOption> = options.iter().filter(|o| !o.keyword.is_empty()).collect();
    if !keyword_opts.is_empty() {
        rows.push(MenuRow::Separator);
        for opt in keyword_opts {
            rows.push(MenuRow::Item(
                MenuItem::new(opt.label.clone(), MenuAction::Option(opt.keyword.clone()))
                    .hint(opt.keyword.clone())
                    .mnemonic(mnemonic_for(&opt.keyword)),
            ));
        }
    }

    rows.push(MenuRow::Separator);
    rows.push(recent_input_submenu(
        recent_inputs
            .iter()
            .take(RECENT_LIMIT)
            .map(|tok| MenuItem::new(tok.clone(), MenuAction::FeedInput(tok.clone())))
            .collect(),
        open_submenu,
    ));
    if has_point_step {
        rows.push(MenuRow::Submenu {
            id: SubmenuId::SnapOverrides,
            label: t!("Snap Overrides").into_owned(),
            items: snap_override_items(),
            open: open_submenu == Some(SubmenuId::SnapOverrides),
        });
    }

    rows.push(MenuRow::Separator);
    rows.extend(navigation_rows(true));
    rows
}

/// Recent Input ▸ — always listed (commercial solutions keep the entry even when there
/// is nothing to recall yet; it is simply disabled then).
fn recent_input_submenu(items: Vec<MenuItem>, open_submenu: Option<SubmenuId>) -> MenuRow {
    MenuRow::Submenu {
        id: SubmenuId::RecentInput,
        label: t!("Recent Input").into_owned(),
        items,
        open: open_submenu == Some(SubmenuId::RecentInput),
    }
}

/// Snap Overrides ▸: one-shot object snaps for the next pick, in the
/// grouping (M2P, then the edge snaps, the curve snaps, the relation snaps,
/// None) plus the settings dialog.
fn snap_override_items() -> Vec<MenuItem> {
    let snap = |label: &str, t: SnapType| {
        MenuItem::new(t!(label).into_owned(), MenuAction::SnapOverride(t)).icon(MenuIcon::Snap(t))
    };
    vec![
        MenuItem::new(t!("Mid Between 2 Points").into_owned(), MenuAction::Mtp).icon(MenuIcon::Mtp),
        snap("Endpoint", SnapType::Endpoint),
        snap("Midpoint", SnapType::Midpoint),
        snap("Intersection", SnapType::Intersection),
        snap("Apparent Intersection", SnapType::ApparentIntersection),
        snap("Extension", SnapType::Extension),
        snap("Center", SnapType::Center),
        snap("Quadrant", SnapType::Quadrant),
        snap("Tangent", SnapType::Tangent),
        snap("Perpendicular", SnapType::Perpendicular),
        snap("Parallel", SnapType::Parallel),
        snap("Node", SnapType::Node),
        snap("Insertion", SnapType::Insertion),
        snap("Nearest", SnapType::Nearest),
        MenuItem::new(t!("None").into_owned(), MenuAction::SnapOverrideNone),
        MenuItem::new(
            t!("Osnap Settings...").into_owned(),
            MenuAction::Command("OSNAP".to_string()),
        ),
    ]
}

/// Pan / Zoom rows. While a command runs they execute transparently (the
/// `'` prefix, as in commercial solutions) so the command resumes afterwards.
fn navigation_rows(transparent: bool) -> Vec<MenuRow> {
    let prefix = if transparent { "'" } else { "" };
    vec![
        MenuRow::Item(
            MenuItem::new(t!("Pan").into_owned(), MenuAction::Command(format!("{prefix}PAN")))
                .icon(MenuIcon::Pan),
        ),
        MenuRow::Item(
            MenuItem::new(
                t!("Zoom").into_owned(),
                MenuAction::Command(format!("{prefix}ZOOM DYNAMIC")),
            )
            .icon(MenuIcon::Zoom),
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn idle_rows(
    has_selection: bool,
    selected_constraint: Option<ConstraintId>,
    isolation_active: bool,
    recent_cmds: &[String],
    clipboard_nonempty: bool,
    undo_label: Option<&str>,
    redo_label: Option<&str>,
    props_open: bool,
    open_submenu: Option<SubmenuId>,
) -> Vec<MenuRow> {
    let cmd = |label: String, name: &str| MenuItem::new(label, MenuAction::Command(name.to_string()));
    let mut rows = Vec::new();

    // ── Repeat / Recent Input ──────────────────────────────────────────
    if let Some(last) = recent_cmds.first() {
        rows.push(MenuRow::Item(
            MenuItem::new(
                t!("Repeat %{last}", last = last.clone()).into_owned(),
                MenuAction::Command(last.to_uppercase()),
            )
            .default(),
        ));
    }
    rows.push(recent_input_submenu(
        recent_cmds
            .iter()
            .take(RECENT_LIMIT)
            .map(|c| MenuItem::new(c.clone(), MenuAction::Command(c.to_uppercase())))
            .collect(),
        open_submenu,
    ));
    rows.push(MenuRow::Separator);

    // ── Clipboard ──────────────────────────────────────────────────────
    rows.push(MenuRow::Submenu {
        id: SubmenuId::Clipboard,
        label: t!("Clipboard").into_owned(),
        items: vec![
            cmd(t!("Cut").into_owned(), "CUTCLIP").enabled(has_selection),
            cmd(t!("Copy").into_owned(), "COPYCLIP").enabled(has_selection),
            cmd(t!("Copy with Base Point").into_owned(), "COPYBASE").enabled(has_selection),
            cmd(t!("Paste").into_owned(), "PASTECLIP").enabled(clipboard_nonempty),
            cmd(t!("Paste as Block").into_owned(), "PASTEBLOCK").enabled(clipboard_nonempty),
            cmd(t!("Paste to Original Coordinates").into_owned(), "PASTEORIG")
                .enabled(clipboard_nonempty),
        ],
        open: open_submenu == Some(SubmenuId::Clipboard),
    });
    rows.push(MenuRow::Separator);

    // ── Undo / Redo ────────────────────────────────────────────────────
    let undo = match undo_label {
        Some(label) => t!("Undo %{label}", label = label.to_string()).into_owned(),
        None => t!("Undo").into_owned(),
    };
    let redo = match redo_label {
        Some(label) => t!("Redo %{label}", label = label.to_string()).into_owned(),
        None => t!("Redo").into_owned(),
    };
    rows.push(MenuRow::Item(
        MenuItem::new(undo, MenuAction::Undo).hint("Ctrl+Z").enabled(undo_label.is_some()),
    ));
    rows.push(MenuRow::Item(
        MenuItem::new(redo, MenuAction::Redo).hint("Ctrl+Y").enabled(redo_label.is_some()),
    ));
    rows.push(MenuRow::Separator);

    // ── Selection edit block (the "Edit" shortcut menu of commercial solutions) ──
    if has_selection {
        rows.push(MenuRow::Item(MenuItem::new(
            t!("Erase").into_owned(),
            MenuAction::DeleteSelected,
        )));
        rows.push(MenuRow::Item(cmd(t!("Move").into_owned(), "MOVE")));
        rows.push(MenuRow::Item(cmd(t!("Copy Selection").into_owned(), "COPY")));
        rows.push(MenuRow::Item(cmd(t!("Scale").into_owned(), "SCALE")));
        rows.push(MenuRow::Item(cmd(t!("Rotate").into_owned(), "ROTATE")));
        rows.push(MenuRow::Item(cmd(t!("Mirror").into_owned(), "MIRROR")));
        rows.push(MenuRow::Submenu {
            id: SubmenuId::DrawOrder,
            label: t!("Draw Order").into_owned(),
            items: vec![
                cmd(t!("Bring to Front").into_owned(), "DRAWORDER F"),
                cmd(t!("Send to Back").into_owned(), "DRAWORDER B"),
                MenuItem::new(
                    t!("Bring Above Object").into_owned(),
                    MenuAction::DrawOrderPickRef(true),
                ),
                MenuItem::new(
                    t!("Send Under Object").into_owned(),
                    MenuAction::DrawOrderPickRef(false),
                ),
            ],
            open: open_submenu == Some(SubmenuId::DrawOrder),
        });
        rows.push(MenuRow::Separator);
    } else if let Some(id) = selected_constraint {
        // A selected constraint-glyph pill gets a minimal edit block — none
        // of the entity actions above apply to it.
        rows.push(MenuRow::Item(MenuItem::new(
            t!("Delete").into_owned(),
            MenuAction::ConstraintDelete(id),
        )));
        rows.push(MenuRow::Separator);
    }

    // ── Isolate ────────────────────────────────────────────────────────
    rows.push(MenuRow::Submenu {
        id: SubmenuId::Isolate,
        label: t!("Isolate").into_owned(),
        items: vec![
            cmd(t!("Isolate Objects").into_owned(), "ISOLATEOBJECTS").enabled(has_selection),
            cmd(t!("Hide Objects").into_owned(), "HIDEOBJECTS").enabled(has_selection),
            cmd(t!("End Object Isolation").into_owned(), "UNISOLATEOBJECTS")
                .enabled(isolation_active),
        ],
        open: open_submenu == Some(SubmenuId::Isolate),
    });
    rows.push(MenuRow::Separator);

    // ── Navigation ─────────────────────────────────────────────────────
    rows.extend(navigation_rows(false));
    // The edit menu is long already; Zoom Extents stays on the idle menu
    // (and on the middle button's double-click), as in commercial solutions.
    if !has_selection {
        rows.push(MenuRow::Item(cmd(t!("Zoom Extents").into_owned(), "ZOOM EXTENTS")));
    }
    rows.push(MenuRow::Separator);

    // ── Selection tools ────────────────────────────────────────────────
    if has_selection {
        rows.push(MenuRow::Item(MenuItem::new(
            t!("Select Similar").into_owned(),
            MenuAction::SelectSimilar,
        )));
        rows.push(MenuRow::Item(MenuItem::new(
            t!("Invert Selection").into_owned(),
            MenuAction::InvertSelection,
        )));
        rows.push(MenuRow::Item(MenuItem::new(
            t!("Deselect All").into_owned(),
            MenuAction::DeselectAll,
        )));
    }
    if !has_selection {
        rows.push(MenuRow::Item(cmd(t!("Select All").into_owned(), "SELECTALL")));
    }
    rows.push(MenuRow::Item(MenuItem::new(
        t!("Quick Select...").into_owned(),
        MenuAction::QuickSelect,
    )));
    rows.push(MenuRow::Separator);

    // ── Panels ─────────────────────────────────────────────────────────
    rows.push(MenuRow::Item(
        MenuItem::new(t!("Properties").into_owned(), MenuAction::Properties).checked(props_open),
    ));
    rows.push(MenuRow::Item(MenuItem::new(
        t!("Options...").into_owned(),
        MenuAction::Options,
    )));
    rows
}

fn grip_rows(grip: &GripMenuContext) -> Vec<MenuRow> {
    let g = |label: String, cmd: GripMenuCmd| MenuItem::new(label, MenuAction::Grip(cmd));
    vec![
        MenuRow::Item(MenuItem::new(t!("Enter").into_owned(), MenuAction::Enter).hint("⏎").default()),
        MenuRow::Separator,
        MenuRow::Item(
            g(t!("Stretch").into_owned(), GripMenuCmd::Stretch)
                .checked(matches!(grip.mode, GripEditMode::Stretch)),
        ),
        MenuRow::Item(g(t!("Move").into_owned(), GripMenuCmd::Move)),
        MenuRow::Item(g(t!("Rotate").into_owned(), GripMenuCmd::Rotate)),
        MenuRow::Item(g(t!("Scale").into_owned(), GripMenuCmd::Scale)),
        MenuRow::Item(g(t!("Mirror").into_owned(), GripMenuCmd::Mirror)),
        MenuRow::Separator,
        MenuRow::Item(g(t!("Base Point").into_owned(), GripMenuCmd::BasePoint)),
        MenuRow::Item(g(t!("Copy").into_owned(), GripMenuCmd::CopyToggle).checked(grip.copy_on)),
        MenuRow::Item(g(t!("Undo").into_owned(), GripMenuCmd::Undo).enabled(grip.moved)),
        MenuRow::Separator,
        MenuRow::Item(g(t!("Exit").into_owned(), GripMenuCmd::Exit).hint("Esc")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Labels are translated at build time, so expectations go through the
    /// same catalog (tests run under whatever UI language the host has).
    fn tl(source: &str) -> String {
        t!(source).into_owned()
    }

    fn labels(menu: &ContextMenu) -> Vec<String> {
        let mut out = Vec::new();
        for row in &menu.rows {
            match row {
                MenuRow::Item(item) => out.push(item.label.clone()),
                MenuRow::Separator => out.push("---".into()),
                MenuRow::Submenu { label, items, open, .. } => {
                    out.push(format!("{label} >"));
                    if *open {
                        out.extend(items.iter().map(|i| format!("  {}", i.label)));
                    }
                }
            }
        }
        out
    }

    fn actions(menu: &ContextMenu) -> Vec<MenuAction> {
        menu.selectable().into_iter().map(|s| s.action).collect()
    }

    fn item_at(menu: &ContextMenu, idx: usize) -> &MenuItem {
        match &menu.rows[idx] {
            MenuRow::Item(item) => item,
            other => panic!("expected item at {idx}, got {other:?}"),
        }
    }

    fn line_ctx() -> MenuContext {
        MenuContext::Command {
            options: vec![CmdOption::new("Close", "C"), CmdOption::new("Undo", "U")],
            has_point_step: true,
            recent_inputs: Vec::new(),
        }
    }

    #[test]
    fn command_menu_lists_options_with_hints() {
        let menu = build_context_menu(&line_ctx(), None);
        // Enter / Cancel, the keywords, then the recall / snap helpers and
        // the navigation rows.
        assert_eq!(
            labels(&menu),
            vec![
                tl("Enter"),
                tl("Cancel"),
                "---".into(),
                tl("Close"),
                tl("Undo"),
                "---".into(),
                format!("{} >", tl("Recent Input")),
                format!("{} >", tl("Snap Overrides")),
                "---".into(),
                tl("Pan"),
                tl("Zoom"),
            ]
        );
        let close = item_at(&menu, 3);
        assert_eq!(close.hint.as_deref(), Some("C"));
        assert_eq!(close.mnemonic, Some('C'));
        assert_eq!(close.action, MenuAction::Option("C".into()));
        assert!(item_at(&menu, 0).default);
        assert_eq!(menu.default_index(), 0);
        assert_eq!(menu.width, MENU_WIDTH_WITH_HINTS);
    }

    #[test]
    fn enter_option_becomes_default_row() {
        let ctx = MenuContext::Command {
            options: vec![CmdOption::new("Arc", "A"), CmdOption::enter("Done")],
            has_point_step: false,
            recent_inputs: Vec::new(),
        };
        let menu = build_context_menu(&ctx, None);
        // The finish option is the Enter row itself — listed once, as "Enter".
        let done = item_at(&menu, 0);
        assert_eq!(done.label, tl("Enter"));
        assert!(done.default);
        assert_eq!(done.action, MenuAction::Enter);
        assert_eq!(done.hint.as_deref(), Some("⏎"));
        assert_eq!(
            actions(&menu)[..4],
            [
                MenuAction::Enter,
                MenuAction::Cancel,
                MenuAction::Option("A".into()),
                MenuAction::ToggleSubmenu(SubmenuId::RecentInput),
            ]
        );
        // A default value shows on the Enter row as its hint (OFFSET <dist>).
        let ctx = MenuContext::Command {
            options: vec![CmdOption::enter("<0.5000>")],
            has_point_step: false,
            recent_inputs: Vec::new(),
        };
        let menu = build_context_menu(&ctx, None);
        assert_eq!(item_at(&menu, 0).hint.as_deref(), Some("<0.5000>"));
    }

    #[test]
    fn command_menu_without_options_has_only_enter_cancel() {
        let ctx = MenuContext::Command {
            options: Vec::new(),
            has_point_step: false,
            recent_inputs: Vec::new(),
        };
        let menu = build_context_menu(&ctx, None);
        assert_eq!(
            actions(&menu),
            vec![
                MenuAction::Enter,
                MenuAction::Cancel,
                MenuAction::ToggleSubmenu(SubmenuId::RecentInput),
                MenuAction::Command("'PAN".into()),
                MenuAction::Command("'ZOOM DYNAMIC".into()),
            ]
        );
        // Nothing to recall yet: the Recent Input header is disabled.
        assert!(!menu.selectable()[2].enabled);
    }

    #[test]
    fn recent_input_submenu_lists_tokens_when_open() {
        let ctx = MenuContext::Command {
            options: Vec::new(),
            has_point_step: false,
            recent_inputs: vec!["10,20".into(), "C".into()],
        };
        let closed = build_context_menu(&ctx, None);
        assert_eq!(
            actions(&closed)[..3],
            [
                MenuAction::Enter,
                MenuAction::Cancel,
                MenuAction::ToggleSubmenu(SubmenuId::RecentInput)
            ]
        );
        assert!(closed.selectable()[2].enabled);
        let open = build_context_menu(&ctx, Some(SubmenuId::RecentInput));
        assert_eq!(
            actions(&open)[..5],
            [
                MenuAction::Enter,
                MenuAction::Cancel,
                MenuAction::ToggleSubmenu(SubmenuId::RecentInput),
                MenuAction::FeedInput("10,20".into()),
                MenuAction::FeedInput("C".into()),
            ]
        );
    }

    #[test]
    fn snap_overrides_submenu_lists_every_object_snap() {
        let ctx = MenuContext::Command {
            options: Vec::new(),
            has_point_step: true,
            recent_inputs: Vec::new(),
        };
        let menu = build_context_menu(&ctx, Some(SubmenuId::SnapOverrides));
        let acts = actions(&menu);
        assert!(acts.contains(&MenuAction::Mtp));
        for &(t, _, _) in crate::snap::ALL_SNAP_MODES {
            assert!(acts.contains(&MenuAction::SnapOverride(t)), "missing {t:?}");
        }
        assert!(acts.contains(&MenuAction::SnapOverrideNone));
        assert!(acts.contains(&MenuAction::Command("OSNAP".into())));
        // An entity-pick step offers no snap overrides.
        let ctx = MenuContext::Command {
            options: Vec::new(),
            has_point_step: false,
            recent_inputs: Vec::new(),
        };
        let menu = build_context_menu(&ctx, None);
        assert!(!actions(&menu).contains(&MenuAction::ToggleSubmenu(SubmenuId::SnapOverrides)));
    }

    fn idle_ctx(has_selection: bool) -> MenuContext {
        MenuContext::Idle {
            has_selection,
            selected_constraint: None,
            isolation_active: false,
            recent_cmds: vec!["LINE".into(), "CIRCLE".into()],
            clipboard_nonempty: false,
            undo_label: Some("LINE".into()),
            redo_label: None,
            props_open: false,
        }
    }

    #[test]
    fn idle_menu_without_selection_has_repeat_default() {
        let menu = build_context_menu(&idle_ctx(false), None);
        let first = item_at(&menu, 0);
        assert_eq!(first.action, MenuAction::Command("LINE".into()));
        assert!(first.default);
        assert_eq!(menu.default_index(), 0);
        let acts = actions(&menu);
        assert!(acts.contains(&MenuAction::Command("PAN".into())));
        assert!(acts.contains(&MenuAction::Options));
        assert!(!acts.contains(&MenuAction::DeleteSelected));
        assert!(!acts.contains(&MenuAction::DeselectAll));
    }

    #[test]
    fn selection_menu_lists_modify_actions() {
        let menu = build_context_menu(&idle_ctx(true), Some(SubmenuId::DrawOrder));
        let acts = actions(&menu);
        for want in [
            MenuAction::DeleteSelected,
            MenuAction::Command("MOVE".into()),
            MenuAction::Command("COPY".into()),
            MenuAction::Command("SCALE".into()),
            MenuAction::Command("ROTATE".into()),
            MenuAction::Command("MIRROR".into()),
            MenuAction::ToggleSubmenu(SubmenuId::DrawOrder),
            MenuAction::Command("DRAWORDER F".into()),
            MenuAction::DrawOrderPickRef(false),
            MenuAction::SelectSimilar,
            MenuAction::DeselectAll,
        ] {
            assert!(acts.contains(&want), "missing {want:?} in {acts:?}");
        }
    }

    #[test]
    fn clipboard_rows_disabled_when_empty() {
        let menu = build_context_menu(&idle_ctx(false), Some(SubmenuId::Clipboard));
        let sel = menu.selectable();
        let paste = sel
            .iter()
            .find(|s| s.action == MenuAction::Command("PASTECLIP".into()))
            .expect("paste row");
        assert!(!paste.enabled);
        let cut = sel
            .iter()
            .find(|s| s.action == MenuAction::Command("CUTCLIP".into()))
            .expect("cut row");
        assert!(!cut.enabled);
    }

    #[test]
    fn undo_row_uses_history_label() {
        let menu = build_context_menu(&idle_ctx(false), None);
        let rows = labels(&menu);
        let undo = t!("Undo %{label}", label = "LINE").into_owned();
        assert!(rows.contains(&undo), "{rows:?}");
        let sel = menu.selectable();
        let redo = sel.iter().find(|s| s.action == MenuAction::Redo).expect("redo row");
        assert!(!redo.enabled);
        let undo = sel.iter().find(|s| s.action == MenuAction::Undo).expect("undo row");
        assert!(undo.enabled);
    }

    #[test]
    fn selectable_skips_separators_and_closed_submenus() {
        let menu = build_context_menu(&idle_ctx(false), None);
        let sel = menu.selectable();
        assert!(sel.iter().all(|s| s.action != MenuAction::Command("PASTECLIP".into())));
        assert!(sel.iter().any(|s| s.header == Some(SubmenuId::Clipboard)));
        let open = build_context_menu(&idle_ctx(false), Some(SubmenuId::Clipboard));
        assert!(open
            .selectable()
            .iter()
            .any(|s| s.action == MenuAction::Command("PASTECLIP".into())));
    }

    #[test]
    fn find_mnemonic_cycles_duplicates() {
        let ctx = MenuContext::Command {
            options: vec![
                CmdOption::new("Angle", "A"),
                CmdOption::new("CEnter", "CE"),
                CmdOption::new("CLose", "CL"),
            ],
            has_point_step: false,
            recent_inputs: Vec::new(),
        };
        let menu = build_context_menu(&ctx, None);
        // rows: Enter(0) Cancel(1) Angle(2) CEnter(3) CLose(4) …
        assert_eq!(menu.find_mnemonic('c', None), Some(3));
        assert_eq!(menu.find_mnemonic('C', Some(3)), Some(4));
        assert_eq!(menu.find_mnemonic('C', Some(4)), Some(3));
        assert_eq!(menu.find_mnemonic('Z', None), None);
    }

    #[test]
    fn default_row_y_accounts_for_separators() {
        let menu = build_context_menu(&line_ctx(), None);
        assert_eq!(menu.default_row_y(), MENU_PAD_TOP);
        let menu = build_context_menu(&idle_ctx(false), None);
        assert_eq!(menu.default_row_y(), MENU_PAD_TOP);
        // 10 items + 3 submenu headers + 6 separators, plus panel padding.
        let expected = MENU_PAD_TOP * 2.0 + 13.0 * MENU_ROW_H + 6.0 * MENU_SEP_H;
        assert_eq!(menu.height(), expected);
    }

    #[test]
    fn grip_menu_marks_current_mode() {
        let menu = build_context_menu(
            &MenuContext::Grip(GripMenuContext {
                mode: GripEditMode::Stretch,
                copy_on: true,
                moved: false,
            }),
            None,
        );
        assert_eq!(item_at(&menu, 0).action, MenuAction::Enter);
        assert!(item_at(&menu, 2).checked);
        let sel = menu.selectable();
        let stretch = sel
            .iter()
            .position(|s| s.action == MenuAction::Grip(GripMenuCmd::Stretch))
            .unwrap();
        assert_eq!(stretch, 1);
        let undo = sel
            .iter()
            .find(|s| s.action == MenuAction::Grip(GripMenuCmd::Undo))
            .unwrap();
        assert!(!undo.enabled);
    }
}
