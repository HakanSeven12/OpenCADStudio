//! The app's own title bar over the ribbon: moving, maximizing, minimizing
//! and closing the main window, and resizing it from its edges.

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::ui::title_bar::{DoubleClick, TitleBarMsg};

impl OpenCADStudio {
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
            TitleBarMsg::DoubleClick => {
                self.title_bar_pressed = false;
                match crate::ui::title_bar::double_click() {
                    DoubleClick::Maximize => self.on_title_bar(TitleBarMsg::ToggleMaximize),
                    DoubleClick::Minimize => iced::window::minimize(id, true),
                    DoubleClick::Nothing => Task::none(),
                }
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
        // As in the app, where the ribbon reads it from `title_bar::drawn`.
        app.ribbon.set_in_title_bar(app.draws_title_bar());
        app
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
        // The toolbar below the ribbon: the menus take its place on top.
        app.show_menu_bar = true;
        let mut layout = app.ribbon.layout().clone();
        layout.qat_below = true;
        app.ribbon.set_layout(layout);
        std::hint::black_box(app.view_main());
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
}
