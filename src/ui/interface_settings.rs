//! Interface Settings: the popover under its button in the title bar (and
//! the INTERFACESETTINGS command) that gathers where the window's parts
//! show: the Ribbon or Toolbar layout, the colour theme, the Quick Access
//! Toolbar, the menu bar, the file tabs, the command line and the status
//! bar, the ribbon's options and the floating toolbars. Every change is
//! made at once; there is nothing to apply.
//!
//! It is built from the window kit only ([`crate::ui::kit::form`]), as
//! mockup B draws it: no grid lines or cell backgrounds, label and control
//! each half a row.

use iced::widget::Column;
use iced::Element;

use crate::app::Message;
use crate::ui::dropdown_list::DropdownList;
use crate::ui::kit;
use crate::ui::kit::form;
use crate::ui::menu_bar::MenuBarPosition;
use crate::ui::ribbon::{MinimizePref, QatPosition};
use crate::ui::style::kit_metrics as km;

/// The open dropdown id of the popover (`Ribbon::open_dropdown`), and the
/// id its button reports its place under.
pub const INTERFACE_SETTINGS_ID: &str = "INTERFACE_SETTINGS";

/// One of the popover's choice lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
    Theme,
    QatPosition,
    MenuBarPosition,
    FileTabs,
    MinimizeTo,
    ToolbarTitle,
}

/// What the popover asks for.
#[derive(Debug, Clone)]
pub enum InterfaceMsg {
    /// The title bar's button: open the popover, or close it.
    Toggle,
    /// Open the popover (INTERFACESETTINGS).
    Open,
    /// Esc, a press outside it or its close button.
    Close,
    /// Open one of its lists, or close the one open (`None`).
    List(Option<List>),
    /// The Toolbar layout (`true`) or the Ribbon layout.
    Layout(bool),
    Theme(String),
    QatShown(bool),
    QatPosition(QatPosition),
    MenuBarShown(bool),
    MenuBarPosition(MenuBarPosition),
    FileTabsShown(bool),
    /// The file tabs above the palettes (`true`) or below.
    FileTabsAbove(bool),
    CommandLineShown(bool),
    StatusBarShown(bool),
    MinimizeTo(MinimizePref),
    PanelTitles(bool),
    Centered(bool),
    WheelTabs(bool),
    /// Show the panel `title` of tab `module` as a floating toolbar, or not.
    Toolbar {
        module: &'static str,
        title: &'static str,
        show: bool,
    },
    /// The toolbars' titles at their side (`true`) or at their bottom.
    ToolbarTitleSide(bool),
    ResetToolbars,
    /// Close the popover and open Options on its Display tab.
    Options,
}

/// The popover's own state: which of its lists is open.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub open_list: Option<List>,
}

/// What the popover shows, read from the app as it stands.
pub struct Snapshot {
    /// The Toolbar layout (the ribbon closed).
    pub toolbar_layout: bool,
    /// The theme names (`config::all_themes`, then Custom) and the current
    /// one.
    pub themes: Vec<String>,
    pub theme: String,
    pub qat_shown: bool,
    pub qat_position: QatPosition,
    pub menu_bar_shown: bool,
    pub menu_bar_position: MenuBarPosition,
    /// The menus are wanted in the title bar but do not fit there now.
    pub menus_fall_back: bool,
    pub file_tabs_shown: bool,
    pub file_tabs_above: bool,
    pub command_line_shown: bool,
    pub status_bar_shown: bool,
    pub minimize_pref: MinimizePref,
    pub panel_titles: bool,
    pub centered: bool,
    pub wheel_tabs: bool,
    /// The toolbars to offer: tab id, panel title, shown.
    pub toolbars: Vec<(&'static str, &'static str, bool)>,
    /// Every floating toolbar has its title at the side.
    pub toolbar_titles_side: bool,
}

fn msg(message: InterfaceMsg) -> Message {
    Message::Interface(message)
}

/// A theme's name as Options shows it.
pub fn theme_label(name: &str) -> String {
    match name {
        "Light" => crate::t!("Light").into_owned(),
        "Dark" => crate::t!("Dark").into_owned(),
        "Custom" => crate::t!("Custom").into_owned(),
        _ => name.to_string(),
    }
}

pub fn qat_position_label(position: QatPosition) -> String {
    match position {
        QatPosition::TitleBar => crate::tr!("interface-settings", "in-title-bar"),
        QatPosition::BeforeTabs => crate::tr!("interface-settings", "before-ribbon-tabs"),
        QatPosition::BelowRibbon => crate::tr!("interface-settings", "below-ribbon"),
    }
}

pub fn menu_bar_position_label(position: MenuBarPosition) -> String {
    match position {
        MenuBarPosition::TitleBar => crate::tr!("interface-settings", "in-title-bar"),
        MenuBarPosition::OwnRow => crate::tr!("interface-settings", "below-title-bar"),
    }
}

fn file_tabs_label(above: bool) -> String {
    if above {
        crate::tr!("interface-settings", "above-palettes")
    } else {
        crate::tr!("interface-settings", "below-palettes")
    }
}

fn minimize_label(pref: MinimizePref) -> String {
    match pref {
        MinimizePref::Cycle => crate::tr!("interface-settings", "minimize-cycle"),
        MinimizePref::Tabs => crate::tr!("interface-settings", "minimize-tabs"),
        MinimizePref::PanelTitles => crate::tr!("interface-settings", "minimize-panel-titles"),
        MinimizePref::PanelButtons => crate::tr!("interface-settings", "minimize-panel-buttons"),
    }
}

fn title_side_label(side: bool) -> String {
    if side {
        crate::tr!("interface-settings", "title-side")
    } else {
        crate::tr!("interface-settings", "title-bottom")
    }
}

/// A choice field for `list`: `options` (label, message), the one shown
/// `current`. While not `enabled` it shows `current` greyed, with nothing
/// to open.
fn choice<'a>(
    state: &State,
    list: List,
    options: Vec<(String, InterfaceMsg)>,
    current: &str,
    enabled: bool,
) -> Element<'a, Message> {
    let open = enabled && state.open_list == Some(list);
    let toggle = msg(InterfaceMsg::List(if open { None } else { Some(list) }));
    let width = kit::dropdown_width(
        options.iter().map(|(label, _)| label.as_str()),
        km::FORM_W / 2.0,
    );
    let selected = options.iter().position(|(label, _)| label == current);
    let rows = options
        .into_iter()
        .map(|(label, message)| {
            let on = label == current;
            kit::dropdown_option(label.clone(), label, msg(message), on)
        })
        .collect();
    let list = DropdownList::new(rows)
        .selected(selected)
        .select_first_by_default(selected.is_some())
        .on_escape(msg(InterfaceMsg::List(None)));
    form::select(current.to_string(), open, toggle, list, width, !enabled)
}

/// The popover.
pub fn view<'a>(state: &State, snap: &Snapshot) -> Element<'a, Message> {
    let mut bands: Column<'a, Message> = form::rows(Vec::new());

    // ── Layout ────────────────────────────────────────────────────────────
    bands = bands.push(form::section(crate::tr!("interface-settings", "layout")));
    bands = bands.push(form::tiles(
        vec![
            (
                false,
                crate::ui::icons::LAYOUT_RIBBON,
                crate::tr!("interface-settings", "ribbon"),
            ),
            (
                true,
                crate::ui::icons::LAYOUT_TOOLBAR,
                crate::tr!("interface-settings", "toolbar"),
            ),
        ],
        snap.toolbar_layout,
        |toolbar| msg(InterfaceMsg::Layout(toolbar)),
    ));

    // ── Window elements ───────────────────────────────────────────────────
    bands = bands.push(form::section(crate::tr!(
        "interface-settings",
        "window-elements"
    )));
    let theme_options = snap
        .themes
        .iter()
        .map(|name| (theme_label(name), InterfaceMsg::Theme(name.clone())))
        .collect();
    bands = bands.push(form::row(
        crate::tr!("interface-settings", "color-theme"),
        choice(
            state,
            List::Theme,
            theme_options,
            &theme_label(&snap.theme),
            true,
        ),
    ));

    // The toolbar stays in the title bar with the Toolbar layout: the
    // choice is kept for the Ribbon layout but cannot change now.
    let qat_shown_position = if snap.toolbar_layout {
        QatPosition::TitleBar
    } else {
        snap.qat_position
    };
    let qat_choice = choice(
        state,
        List::QatPosition,
        [
            QatPosition::TitleBar,
            QatPosition::BeforeTabs,
            QatPosition::BelowRibbon,
        ]
        .into_iter()
        .map(|position| {
            (
                qat_position_label(position),
                InterfaceMsg::QatPosition(position),
            )
        })
        .collect(),
        &qat_position_label(qat_shown_position),
        snap.qat_shown && !snap.toolbar_layout,
    );
    let qat_row = form::check_row(
        crate::tr!("interface-settings", "quick-access"),
        snap.qat_shown,
        Some(msg(InterfaceMsg::QatShown(!snap.qat_shown))),
        qat_choice,
    );
    // The full name, and why the place is fixed with the Toolbar layout.
    let qat_tip = if snap.toolbar_layout && snap.qat_shown {
        format!(
            "{}\n{}",
            crate::tr!("interface-settings", "quick-access-toolbar"),
            crate::tr!("interface-settings", "qat-fixed")
        )
    } else {
        crate::tr!("interface-settings", "quick-access-toolbar")
    };
    bands = bands.push(kit::tooltip(qat_row, qat_tip, kit::TipPosition::Bottom));

    let menu_choice = choice(
        state,
        List::MenuBarPosition,
        [MenuBarPosition::TitleBar, MenuBarPosition::OwnRow]
            .into_iter()
            .map(|position| {
                (
                    menu_bar_position_label(position),
                    InterfaceMsg::MenuBarPosition(position),
                )
            })
            .collect(),
        &menu_bar_position_label(snap.menu_bar_position),
        snap.menu_bar_shown,
    );
    bands = bands.push(form::check_row(
        crate::tr!("interface-settings", "menu-bar"),
        snap.menu_bar_shown,
        Some(msg(InterfaceMsg::MenuBarShown(!snap.menu_bar_shown))),
        menu_choice,
    ));
    if snap.menu_bar_shown && snap.menus_fall_back {
        bands = bands.push(form::hint(
            crate::tr!("interface-settings", "menus-own-row"),
            true,
        ));
    }

    let tabs_choice = choice(
        state,
        List::FileTabs,
        [true, false]
            .into_iter()
            .map(|above| (file_tabs_label(above), InterfaceMsg::FileTabsAbove(above)))
            .collect(),
        &file_tabs_label(snap.file_tabs_above),
        snap.file_tabs_shown,
    );
    bands = bands.push(form::check_row(
        crate::tr!("interface-settings", "file-tabs"),
        snap.file_tabs_shown,
        Some(msg(InterfaceMsg::FileTabsShown(!snap.file_tabs_shown))),
        tabs_choice,
    ));
    bands = bands.push(form::check(
        crate::tr!("interface-settings", "command-line"),
        snap.command_line_shown,
        Some(msg(InterfaceMsg::CommandLineShown(
            !snap.command_line_shown,
        ))),
    ));
    bands = bands.push(form::check(
        crate::tr!("interface-settings", "status-bar"),
        snap.status_bar_shown,
        Some(msg(InterfaceMsg::StatusBarShown(!snap.status_bar_shown))),
    ));

    if snap.toolbar_layout {
        // ── Toolbars ──────────────────────────────────────────────────────
        bands = bands.push(form::section(crate::tr!("interface-settings", "toolbars")));
        // Two columns, so the list stays short.
        let toolbars = snap
            .toolbars
            .iter()
            .map(|&(module, title, shown)| {
                form::check(
                    crate::t!(title).into_owned(),
                    shown,
                    Some(msg(InterfaceMsg::Toolbar {
                        module,
                        title,
                        show: !shown,
                    })),
                )
            })
            .collect();
        bands = bands.push(form::two_columns(toolbars));
        bands = bands.push(form::row(
            crate::tr!("interface-settings", "toolbar-title"),
            choice(
                state,
                List::ToolbarTitle,
                [false, true]
                    .into_iter()
                    .map(|side| (title_side_label(side), InterfaceMsg::ToolbarTitleSide(side)))
                    .collect(),
                &title_side_label(snap.toolbar_titles_side),
                true,
            ),
        ));
        bands = bands.push(
            iced::widget::container(form::button(
                crate::tr!("interface-settings", "reset-toolbars"),
                msg(InterfaceMsg::ResetToolbars),
            ))
            .padding(
                iced::Padding::ZERO
                    .top(km::FORM_BUTTON_GAP)
                    .left(km::FORM_LABEL_X),
            ),
        );
    } else {
        // ── Ribbon ────────────────────────────────────────────────────────
        bands = bands.push(form::section(crate::tr!("interface-settings", "ribbon")));
        bands = bands.push(form::row(
            crate::tr!("interface-settings", "minimize-to"),
            choice(
                state,
                List::MinimizeTo,
                [
                    MinimizePref::Cycle,
                    MinimizePref::Tabs,
                    MinimizePref::PanelTitles,
                    MinimizePref::PanelButtons,
                ]
                .into_iter()
                .map(|pref| (minimize_label(pref), InterfaceMsg::MinimizeTo(pref)))
                .collect(),
                &minimize_label(snap.minimize_pref),
                true,
            ),
        ));
        bands = bands.push(form::check(
            crate::tr!("interface-settings", "panel-titles"),
            snap.panel_titles,
            Some(msg(InterfaceMsg::PanelTitles(!snap.panel_titles))),
        ));
        bands = bands.push(form::check(
            crate::tr!("interface-settings", "center-tabs"),
            snap.centered,
            Some(msg(InterfaceMsg::Centered(!snap.centered))),
        ));
        bands = bands.push(form::check(
            crate::tr!("interface-settings", "wheel-tabs"),
            snap.wheel_tabs,
            Some(msg(InterfaceMsg::WheelTabs(!snap.wheel_tabs))),
        ));
    }
    // Options… ends the popover as a button at the right of its footer.
    let footer = form::footer(vec![form::button(
        crate::tr!("interface-settings", "options"),
        msg(InterfaceMsg::Options),
    )]);
    form::popover(
        crate::tr!("interface-settings", "title"),
        msg(InterfaceMsg::Close),
        bands,
        Some(footer),
    )
}
