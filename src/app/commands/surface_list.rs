//! The surface list window of the Create Points > Surface commands: Enter at
//! their surface prompt opens it; OK answers the prompt with the surface
//! picked, Cancel ends the command (measured).

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::ui::window::surface_list::{SurfaceList, SurfaceListMsg};

impl OpenCADStudio {
    /// After a command result: a command at its surface list prompt opens
    /// the window, with the surface it shows picked.
    pub(in crate::app) fn open_surface_list_if_asked(&mut self) {
        if self.active_modal.is_some() || self.script_running {
            return;
        }
        let i = self.active_tab;
        let Some((names, picked)) = self.tabs[i].active_cmd.as_ref().and_then(|c| c.surface_list()) else {
            return;
        };
        self.surface_list = Some(SurfaceList { names, picked });
        self.active_modal = Some(crate::app::ModalKind::SurfaceList);
    }

    pub(in crate::app) fn on_surface_list(&mut self, message: SurfaceListMsg) -> Task<Message> {
        match message {
            SurfaceListMsg::Pick(k) => {
                if let Some(state) = self.surface_list.as_mut() {
                    state.picked = k;
                }
            }
            SurfaceListMsg::Help => self
                .command_line
                .push_info(crate::t!("Lists the drawing's surfaces; the points take their elevation from the one picked.").as_ref()),
            // The window steps aside for a pick in the drawing; it comes
            // back with the surface picked (measured).
            SurfaceListMsg::FromDrawing => {
                if self.surface_list.take().is_some() {
                    self.close_active_modal();
                    return self.feed_active_cmd(crate::modules::civil::point_surface::PICK_SURFACE);
                }
            }
            SurfaceListMsg::Ok => {
                let Some(state) = self.surface_list.take() else { return Task::none() };
                self.close_active_modal();
                if let Some(name) = state.names.get(state.picked) {
                    return self.feed_active_cmd(name);
                }
            }
        }
        Task::none()
    }
}
