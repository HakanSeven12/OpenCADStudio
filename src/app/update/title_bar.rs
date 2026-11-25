//! The app's own title bar over the ribbon: moving, maximizing, minimizing
//! and closing the main window, and resizing it from its edges.

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::ui::title_bar::TitleBarMsg;

impl OpenCADStudio {
    /// Bring the main window's system frame, the ribbon and the maximize
    /// button in line with `draws_title_bar`, after anything that may have
    /// changed it: the Options checkbox, Restore Defaults, a discarded
    /// Options session.
    pub(in crate::app) fn sync_window_frame(&mut self) -> Task<Message> {
        let draws = self.draws_title_bar();
        self.ribbon.set_in_title_bar(draws);
        let Some(id) = self.main_window else {
            return Task::none();
        };
        if self.frame_shown != draws {
            return Task::none();
        }
        self.frame_shown = !draws;
        // The window may have been restored meanwhile: the button and the
        // resize edges follow what the system reports.
        Task::batch([
            iced::window::toggle_decorations(id),
            iced::window::is_maximized(id)
                .map(|max| Message::TitleBar(TitleBarMsg::Maximized(max))),
        ])
    }

    pub(in crate::app) fn on_title_bar(&mut self, msg: TitleBarMsg) -> Task<Message> {
        let Some(id) = self.main_window else {
            return Task::none();
        };
        match msg {
            TitleBarMsg::Press => {
                self.title_bar_pressed = true;
                Task::none()
            }
            // The first move of a press hands the drag to the system, which
            // also snaps the window to the screen's edges.
            TitleBarMsg::Move if self.title_bar_pressed => {
                self.title_bar_pressed = false;
                iced::window::drag(id)
            }
            TitleBarMsg::Move => Task::none(),
            TitleBarMsg::Release => {
                self.title_bar_pressed = false;
                Task::none()
            }
            TitleBarMsg::ToggleMaximize => {
                self.title_bar_pressed = false;
                self.main_maximized = !self.main_maximized;
                iced::window::toggle_maximize(id)
            }
            TitleBarMsg::Minimize => iced::window::minimize(id, true),
            // As the system's ✕: unsaved drawings are asked about first.
            TitleBarMsg::Close => self.update(Message::WindowCloseRequested(id)),
            TitleBarMsg::Resize(direction) => iced::window::drag_resize(id, direction),
            TitleBarMsg::Maximized(maximized) => {
                self.main_maximized = maximized;
                Task::none()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::title_bar::TitleBarMsg;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.main_window = Some(iced::window::Id::unique());
        app.frame_shown = !app.draws_title_bar();
        app
    }

    #[test]
    fn a_discarded_options_session_takes_the_frame_back() {
        let mut app = app();
        assert!(app.draws_title_bar() && !app.frame_shown);
        let _ = app.update_root(Message::OptionsOpen);
        let _ = app.update_root(Message::SystemTitleBarChanged(true));
        assert!(app.frame_shown, "the system frame is back");
        // Closed without Apply: the app's title bar returns, the frame goes.
        let _ = app.update_root(Message::OptionsCloseDiscard);
        assert!(!app.system_title_bar);
        assert!(app.draws_title_bar() && !app.frame_shown);
        // Restore Defaults after a change does the same.
        let _ = app.update_root(Message::OptionsOpen);
        let _ = app.update_root(Message::SystemTitleBarChanged(true));
        let _ = app.update_root(Message::OptionsRestoreDefaults);
        assert!(app.draws_title_bar() && !app.frame_shown);
    }

    #[test]
    fn the_window_frame_builds_under_a_dialog_and_maximized() {
        let mut app = app();
        for maximized in [true, false] {
            app.main_maximized = maximized;
            std::hint::black_box(app.view_main());
            let _ = app.update_root(Message::OptionsOpen);
            std::hint::black_box(app.view_main());
            let _ = app.update_root(Message::OptionsCloseDiscard);
        }
        // A clean screen with the menu bar keeps the title bar on top.
        app.clean_screen = true;
        app.show_menu_bar = true;
        std::hint::black_box(app.view_main());
    }

    #[test]
    fn a_press_moves_the_window_on_its_first_move_only() {
        let mut app = app();
        let _ = app.update_root(Message::TitleBar(TitleBarMsg::Press));
        assert!(app.title_bar_pressed);
        let _ = app.update_root(Message::TitleBar(TitleBarMsg::Move));
        assert!(!app.title_bar_pressed, "the drag went to the system");
        // A press that ends, or leaves the bar, moves nothing later.
        let _ = app.update_root(Message::TitleBar(TitleBarMsg::Press));
        let _ = app.update_root(Message::TitleBar(TitleBarMsg::Release));
        assert!(!app.title_bar_pressed);
    }

    #[test]
    fn the_maximize_button_follows_the_window() {
        let mut app = app();
        let _ = app.update_root(Message::TitleBar(TitleBarMsg::Maximized(false)));
        assert!(!app.main_maximized);
        let _ = app.update_root(Message::TitleBar(TitleBarMsg::ToggleMaximize));
        assert!(app.main_maximized);
    }

    #[test]
    fn the_system_title_bar_setting_is_saved_and_restored() {
        let mut app = app();
        assert!(!app.system_title_bar, "the app's own title bar by default");
        let _ = app.update_root(Message::SystemTitleBarChanged(true));
        assert!(!app.draws_title_bar());
        let saved = app
            .last_saved_config
            .clone()
            .expect("the change is saved")
            .settings;
        assert!(saved.system_title_bar);
        let mut next = OpenCADStudio::new_for_test();
        next.apply_settings(&saved);
        assert!(next.system_title_bar, "the next session keeps it");
    }
}
