//! Interface Settings: what the popover's rows do, the Ribbon / Toolbar
//! layout switch (also WSCURRENT's) and the snapshot the popover shows.

use iced::Task;

use crate::app::settings::MenuBarPosition;
use crate::app::{Message, OpenCADStudio};
use crate::ui::interface_settings::{InterfaceMsg, Snapshot, INTERFACE_SETTINGS_ID};
use crate::ui::ribbon::RibbonLayout;

/// The theme names the popover offers: every built-in theme, then Custom.
/// Built once; the list does not change while the app runs.
fn theme_names() -> &'static [String] {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        crate::app::config::all_themes()
            .iter()
            .map(ToString::to_string)
            .chain(std::iter::once("Custom".to_string()))
            .collect()
    })
}

impl OpenCADStudio {
    pub(in crate::app) fn on_interface(&mut self, message: InterfaceMsg) -> Task<Message> {
        // A pick from one of its lists closes that list; the popover stays.
        self.interface_settings.open_list = None;
        match message {
            InterfaceMsg::Toggle => {
                let open = self.interface_settings_open();
                self.ribbon.close_dropdown();
                if !open {
                    self.ribbon.open_dropdown = Some(INTERFACE_SETTINGS_ID.to_string());
                }
            }
            InterfaceMsg::Open => {
                self.ribbon.close_dropdown();
                self.ribbon.open_dropdown = Some(INTERFACE_SETTINGS_ID.to_string());
            }
            InterfaceMsg::Close => {
                if self.interface_settings_open() {
                    self.ribbon.close_dropdown();
                }
            }
            InterfaceMsg::List(list) => self.interface_settings.open_list = list,
            InterfaceMsg::Layout(toolbar) => self.set_interface_layout(toolbar),
            InterfaceMsg::Theme(name) => return self.update(Message::OptionsThemeChanged(name)),
            InterfaceMsg::QatShown(shown) => self.edit_ribbon_layout(|layout| {
                layout.qat_hidden = !shown;
            }),
            InterfaceMsg::QatPosition(position) => self.edit_ribbon_layout(|layout| {
                layout.qat_position = position;
            }),
            InterfaceMsg::MenuBarShown(shown) => {
                self.show_menu_bar = shown;
                self.persist_settings_if_changed();
            }
            InterfaceMsg::MenuBarPosition(position) => {
                self.menu_bar_position = position;
                self.persist_settings_if_changed();
            }
            InterfaceMsg::FileTabsShown(shown) => {
                if self.show_file_tabs != shown {
                    return self.update(Message::ToggleFileTabs);
                }
            }
            InterfaceMsg::FileTabsAbove(above) => {
                self.file_tabs_above_docks = above;
                self.save_config();
            }
            InterfaceMsg::CommandLineShown(shown) => {
                self.command_line_hidden = !shown;
                self.persist_settings_if_changed();
            }
            InterfaceMsg::StatusBarShown(shown) => {
                self.status_bar_hidden = !shown;
                self.persist_settings_if_changed();
            }
            InterfaceMsg::MinimizeTo(pref) => self.edit_ribbon_layout(|layout| {
                layout.minimize_pref = pref;
            }),
            InterfaceMsg::PanelTitles(on) => self.edit_ribbon_layout(|layout| {
                layout.show_panel_titles = on;
            }),
            InterfaceMsg::Centered(on) => return self.update(Message::RibbonCenteredChanged(on)),
            InterfaceMsg::WheelTabs(on) => {
                return self.update(Message::RibbonWheelTabsChanged(on));
            }
            InterfaceMsg::Toolbar {
                module,
                title,
                show,
            } => {
                let origin = self.toolbar_origin();
                if self.ribbon.show_toolbar(module, title, show, origin) {
                    self.save_config();
                }
            }
            InterfaceMsg::ToolbarTitleSide(side) => {
                if self.ribbon.set_toolbar_titles_side(side) {
                    self.save_config();
                }
            }
            InterfaceMsg::ResetToolbars => {
                let origin = self.toolbar_origin();
                self.ribbon.reset_toolbars(origin);
                self.save_config();
            }
            InterfaceMsg::Options => {
                self.ribbon.close_dropdown();
                self.options_tab = crate::ui::window::options::OptionsTab::Display;
                return self.update(Message::OptionsOpen);
            }
        }
        Task::none()
    }

    /// Whether the Interface Settings popover is open.
    pub(in crate::app) fn interface_settings_open(&self) -> bool {
        self.ribbon.open_dropdown.as_deref() == Some(INTERFACE_SETTINGS_ID)
    }

    /// Change the ribbon's saved layout with `edit` and save it.
    fn edit_ribbon_layout(&mut self, edit: impl FnOnce(&mut RibbonLayout)) {
        let mut layout = self.ribbon.layout().clone();
        edit(&mut layout);
        if layout != *self.ribbon.layout() {
            self.ribbon.set_layout(layout);
            self.save_config();
        }
    }

    /// The Toolbar layout (`toolbar`) or the Ribbon layout. Toolbar closes
    /// the ribbon and shows the menu bar; each layout keeps its own floating
    /// toolbars, and the first time Toolbar shows the default ones. RIBBON
    /// and RIBBONCLOSE only open or close the ribbon.
    pub(in crate::app) fn set_interface_layout(&mut self, toolbar: bool) {
        self.ribbon_closed = toolbar;
        self.ribbon.closed = toolbar;
        // Also when the ribbon was already closed (RIBBONCLOSE): the sets
        // follow the layout, and Toolbar the first time gets its defaults.
        let origin = self.toolbar_origin();
        self.ribbon.switch_toolbar_set(toolbar, origin);
        if toolbar {
            self.show_menu_bar = true;
        }
        self.persist_settings_if_changed();
    }

    /// What the popover shows.
    pub(in crate::app) fn interface_snapshot(&self) -> Snapshot {
        let layout = self.ribbon.layout();
        let toolbar_layout = self.ribbon_closed;
        let is_start = self.tabs[self.active_tab].is_start;
        let menus_fall_back = self.menu_bar_position == MenuBarPosition::TitleBar
            && self.draws_title_bar()
            && !self
                .ribbon
                .menus_fit_title_bar(self.ribbon_closed || self.clean_screen, is_start);
        // The Home tab's panels, then any other toolbar shown.
        let toolbars = self
            .ribbon
            .toolbars()
            .into_iter()
            .filter(|(module, _, shown)| *module == "draw" || *shown)
            .collect();
        let floating = &layout.floating;
        Snapshot {
            toolbar_layout,
            themes: theme_names().to_vec(),
            theme: self.ui_theme.name.clone(),
            qat_shown: !layout.qat_hidden,
            qat_position: layout.qat_position,
            menu_bar_shown: self.show_menu_bar,
            menu_bar_position: self.menu_bar_position,
            menus_fall_back,
            file_tabs_shown: self.show_file_tabs,
            file_tabs_above: self.file_tabs_above_docks,
            command_line_shown: !self.command_line_hidden,
            status_bar_shown: !self.status_bar_hidden,
            minimize_pref: layout.minimize_pref,
            panel_titles: layout.show_panel_titles,
            centered: layout.centered,
            wheel_tabs: layout.wheel_switches_tabs,
            toolbars,
            toolbar_titles_side: !floating.is_empty()
                && floating.iter().all(|panel| panel.extras_right),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interface_settings::InterfaceMsg as M;
    use crate::ui::ribbon::QatPosition;

    fn app() -> OpenCADStudio {
        OpenCADStudio::new_for_test()
    }

    fn shown_toolbars(app: &OpenCADStudio) -> Vec<&'static str> {
        app.ribbon
            .toolbars()
            .into_iter()
            .filter(|(_, _, shown)| *shown)
            .map(|(_, title, _)| title)
            .collect()
    }

    #[test]
    fn the_popover_opens_toggles_and_closes() {
        let mut app = app();
        let _ = app.update(Message::Interface(M::Toggle));
        assert!(app.interface_settings_open());
        let _ = app.update(Message::Interface(M::Toggle));
        assert!(!app.interface_settings_open());
        let _ = app.update(Message::Interface(M::Open));
        assert!(app.interface_settings_open());
        // A press outside it closes it as any ribbon dropdown.
        let _ = app.update(Message::CloseRibbonDropdown);
        assert!(!app.interface_settings_open());
        // INTERFACESETTINGS opens it.
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("INTERFACESETTINGS");
        assert!(app.interface_settings_open());
        let _ = app.update(Message::Interface(M::Close));
        assert!(!app.interface_settings_open());
        // Esc closes an open list first, then the popover.
        let _ = app.update(Message::Interface(M::Open));
        let _ = app.update(Message::Interface(M::List(Some(
            crate::ui::interface_settings::List::Theme,
        ))));
        let _ = app.on_command_escape();
        assert!(app.interface_settings_open());
        assert!(app.interface_settings.open_list.is_none());
        let _ = app.on_command_escape();
        assert!(!app.interface_settings_open());
    }

    #[test]
    fn the_toolbar_layout_seeds_once_and_each_layout_keeps_its_toolbars() {
        let mut app = app();
        app.ribbon.set_window_size((1600.0, 1000.0));
        let _ = app.update(Message::Interface(M::Layout(true)));
        assert!(app.ribbon_closed);
        assert!(app.show_menu_bar);
        assert_eq!(
            shown_toolbars(&app),
            ["Draw", "Modify", "Layers", "Properties"]
        );
        assert!(app.ribbon.layout().toolbar_layout_seeded);

        // Move one, go to Ribbon and back: it is where it was left.
        let mut layout = app.ribbon.layout().clone();
        layout.floating[0].x = 321.0;
        app.ribbon.set_layout(layout);
        let _ = app.update(Message::Interface(M::Layout(false)));
        assert!(!app.ribbon_closed);
        assert!(shown_toolbars(&app).is_empty());
        let _ = app.update(Message::Interface(M::Layout(true)));
        assert_eq!(app.ribbon.layout().floating[0].x, 321.0);

        // Closing every toolbar does not bring the defaults back.
        for title in ["Draw", "Modify", "Layers", "Properties"] {
            let _ = app.update(Message::Interface(M::Toolbar {
                module: "draw",
                title,
                show: false,
            }));
        }
        let _ = app.update(Message::Interface(M::Layout(false)));
        let _ = app.update(Message::Interface(M::Layout(true)));
        assert!(shown_toolbars(&app).is_empty());

        // Reset toolbars does, with their titles at the bottom.
        let _ = app.update(Message::Interface(M::ToolbarTitleSide(true)));
        let _ = app.update(Message::Interface(M::ResetToolbars));
        assert_eq!(shown_toolbars(&app).len(), 4);
        assert!(app
            .ribbon
            .layout()
            .floating
            .iter()
            .all(|panel| !panel.extras_right));
    }

    #[test]
    fn toolbar_after_ribbonclose_still_gets_its_defaults_and_keeps_the_sets_apart() {
        let mut app = app();
        app.ribbon.set_window_size((1600.0, 1000.0));
        app.automation_op(r#"{"op":"new"}"#);
        // A panel floats in the Ribbon layout, then RIBBONCLOSE.
        let origin = app.toolbar_origin();
        assert!(app.ribbon.show_toolbar("draw", "Block", true, origin));
        let _ = app.run_command_line("RIBBONCLOSE");
        assert!(app.ribbon_closed);

        // Toolbar: its own defaults, the Ribbon layout's panel set aside.
        let _ = app.run_command_line("WSCURRENT CLASSIC");
        assert_eq!(
            shown_toolbars(&app),
            ["Draw", "Modify", "Layers", "Properties"]
        );
        // Ribbon brings the Ribbon layout's panel back.
        let _ = app.update(Message::Interface(M::Layout(false)));
        assert_eq!(shown_toolbars(&app), ["Block"]);
        let _ = app.update(Message::Interface(M::Layout(true)));
        assert_eq!(shown_toolbars(&app).len(), 4);
    }

    #[test]
    fn interfacesettings_opens_the_popover_on_the_start_tab_too() {
        let mut app = app();
        assert!(app.tabs[app.active_tab].is_start);
        let _ = app.update(Message::MenuCommand("INTERFACESETTINGS".into()));
        assert!(app.interface_settings_open());
    }

    #[test]
    fn docked_toolbars_stay_with_their_layout_and_reset_undocks_them() {
        use crate::ui::ribbon::ToolbarEdge;
        let mut app = app();
        app.ribbon.set_window_size((1600.0, 1000.0));
        let _ = app.update(Message::Interface(M::Layout(true)));
        assert!(app.ribbon.dock_toolbar("draw", "Draw", ToolbarEdge::Left));
        assert_eq!(app.ribbon.layout().docked.len(), 1);

        // The Ribbon layout has none docked; back on Toolbar it is there.
        let _ = app.update(Message::Interface(M::Layout(false)));
        assert!(app.ribbon.layout().docked.is_empty());
        let _ = app.update(Message::Interface(M::Layout(true)));
        assert_eq!(app.ribbon.layout().docked.len(), 1);

        // Reset toolbars: the defaults float again, nothing docked.
        let _ = app.update(Message::Interface(M::ResetToolbars));
        assert!(app.ribbon.layout().docked.is_empty());
        assert_eq!(app.ribbon.layout().floating.len(), 4);
    }

    #[test]
    fn the_toolbar_layout_keeps_the_quick_access_place_for_the_ribbon() {
        let mut app = app();
        let _ = app.update(Message::Interface(M::QatPosition(QatPosition::BelowRibbon)));
        let _ = app.update(Message::Interface(M::Layout(true)));
        assert_eq!(
            app.ribbon.qat_placement(app.ribbon_closed),
            Some(QatPosition::TitleBar)
        );
        let snap = app.interface_snapshot();
        assert_eq!(snap.qat_position, QatPosition::BelowRibbon);
        let _ = app.update(Message::Interface(M::Layout(false)));
        assert_eq!(
            app.ribbon.qat_placement(app.ribbon_closed),
            Some(QatPosition::BelowRibbon)
        );
        let _ = app.update(Message::Interface(M::QatShown(false)));
        assert_eq!(app.ribbon.qat_placement(false), None);
    }

    #[test]
    fn window_elements_switch_and_are_saved() {
        let mut app = app();
        let _ = app.update(Message::Interface(M::StatusBarShown(false)));
        let _ = app.update(Message::Interface(M::CommandLineShown(false)));
        let _ = app.update(Message::Interface(M::MenuBarShown(true)));
        let _ = app.update(Message::Interface(M::MenuBarPosition(
            MenuBarPosition::TitleBar,
        )));
        let _ = app.update(Message::Interface(M::FileTabsAbove(true)));
        let _ = app.update(Message::Interface(M::PanelTitles(false)));
        let _ = app.update(Message::Interface(M::MinimizeTo(
            crate::ui::ribbon::MinimizePref::PanelTitles,
        )));
        let saved = app.current_config();
        assert!(saved.settings.status_bar_hidden);
        assert!(saved.settings.command_line_hidden);
        assert!(saved.settings.show_menu_bar);
        assert_eq!(saved.settings.menu_bar_position, MenuBarPosition::TitleBar);
        assert!(saved.file_tabs_above_docks);
        assert!(!saved.ribbon.layout.show_panel_titles);
        assert_eq!(
            saved.ribbon.layout.minimize_pref,
            crate::ui::ribbon::MinimizePref::PanelTitles
        );
        // STATUSBAR reads and writes the same switch.
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("STATUSBAR 1");
        assert!(!app.status_bar_hidden);
        let _ = app.run_command_line("STATUSBAR 0");
        assert!(app.status_bar_hidden);
        // The popover builds in both layouts, every list open.
        for toolbar in [false, true] {
            let _ = app.update(Message::Interface(M::Layout(toolbar)));
            let _ = app.update(Message::Interface(M::Open));
            for list in [
                None,
                Some(crate::ui::interface_settings::List::Theme),
                Some(crate::ui::interface_settings::List::QatPosition),
                Some(crate::ui::interface_settings::List::MinimizeTo),
                Some(crate::ui::interface_settings::List::ToolbarTitle),
            ] {
                app.interface_settings.open_list = list;
                std::hint::black_box(crate::ui::interface_settings::view(
                    &app.interface_settings,
                    &app.interface_snapshot(),
                ));
                std::hint::black_box(app.view_main());
            }
        }
    }
}
