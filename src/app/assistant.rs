//! App side of the Assistant pallet: the ASSISTANT command, its messages and
//! its place in the dock. The pallet itself lives in
//! [`crate::ui::window::assistant`].

use iced::{Element, Task};

use super::{Message, OpenCADStudio};
use crate::ui::dock::PanelId;
use crate::ui::window::assistant::{AssistantMsg, INPUT_ID};

impl OpenCADStudio {
    pub(in crate::app) fn assistant_view<'a>(
        &'a self,
        width: f32,
        chrome: crate::ui::dock::Chrome,
    ) -> Element<'a, Message> {
        crate::ui::window::assistant::view(&self.assistant, &self.active_theme, width, chrome)
    }

    pub(in crate::app) fn on_assistant(&mut self, msg: AssistantMsg) -> Task<Message> {
        self.assistant.update(msg)
    }

    /// ASSISTANT: show the pallet (docked on the right the first time)
    /// with its input focused, or close it when it is showing.
    pub(in crate::app) fn toggle_assistant(&mut self) -> Task<Message> {
        if self.dock_panel_visible(PanelId::Assistant) {
            return self.on_dock(crate::ui::dock::DockMsg::Close(PanelId::Assistant));
        }
        self.assistant.opened();
        self.dock_open_at_default(PanelId::Assistant);
        self.save_config();
        iced::widget::operation::focus(iced::widget::Id::new(INPUT_ID))
    }
}
