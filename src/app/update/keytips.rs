//! Ribbon key tips: a lone Alt opens them, typed codes walk the ribbon and
//! Esc goes back one level.

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::ui::ribbon::{KeyTipInput, KeyTipStep};

impl OpenCADStudio {
    pub(in crate::app) fn on_keytip(&mut self, input: KeyTipInput) -> Task<Message> {
        match input {
            KeyTipInput::AltTap if self.ribbon.keytips_active() => self.hide_keytips(),
            KeyTipInput::AltTap => {
                if self.active_modal.is_some() || self.clean_screen || self.ribbon_closed {
                    return Task::none();
                }
                // A focused field other than the command line keeps Alt.
                iced::advanced::widget::operate(
                    iced::advanced::widget::operation::focusable::count(),
                )
                .then(|count| {
                    if count.focused.is_none() {
                        return Task::done(Message::KeyTip(KeyTipInput::Open { refocus: false }));
                    }
                    iced::widget::operation::is_focused(iced::widget::Id::new(
                        crate::ui::command_line::CMD_INPUT_ID,
                    ))
                    .map(|command_line| {
                        if command_line {
                            Message::KeyTip(KeyTipInput::Open { refocus: true })
                        } else {
                            Message::Noop
                        }
                    })
                })
            }
            KeyTipInput::Open { refocus } => {
                self.ribbon.show_keytips(refocus);
                // Typed codes must not reach the command line.
                if refocus {
                    self.unfocus_widgets()
                } else {
                    Task::none()
                }
            }
            KeyTipInput::Hide => self.hide_keytips(),
            KeyTipInput::Escape => {
                if self.ribbon.keytip_escape() {
                    Task::none()
                } else {
                    self.hide_keytips()
                }
            }
            KeyTipInput::Char(_) | KeyTipInput::Enter => {
                let ch = match &input {
                    KeyTipInput::Char(ch) => Some(ch.as_str()),
                    _ => None,
                };
                match self.ribbon.keytip_type(ch) {
                    KeyTipStep::Ignore | KeyTipStep::Stay => Task::none(),
                    KeyTipStep::Open(message) => self.update(message),
                    KeyTipStep::Run(message) => {
                        let refocus = self.hide_keytips();
                        Task::batch([refocus, self.update(message)])
                    }
                }
            }
        }
    }

    /// Close the key tips, returning the focus to the command line if it had
    /// it.
    pub(in crate::app) fn hide_keytips(&mut self) -> Task<Message> {
        if self.ribbon.hide_keytips() {
            self.focus_cmd_input()
        } else {
            Task::none()
        }
    }
}
