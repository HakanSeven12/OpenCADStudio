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
                let i = self.active_tab;
                // The Start tab has no drawing for the ribbon or the Quick
                // Access Toolbar, so no key tips either.
                if self.tabs[i].is_start
                    || self.active_modal.is_some()
                    || self.clean_screen
                    || self.ribbon_closed
                    || self.tabs[i].active_cmd.is_some()
                    || self.tabs[i].suspended_cmd.is_some()
                    || self.control.get_point.is_some()
                    || self.control.user_select.is_some()
                {
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
            KeyTipInput::Open { .. } if self.tabs[self.active_tab].is_start => Task::none(),
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

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;
    use crate::ui::ribbon::KeyTipInput;

    #[test]
    fn no_key_tips_on_the_start_tab() {
        let mut app = OpenCADStudio::new_for_test();
        assert!(app.tabs[app.active_tab].is_start);
        let _ = app.on_keytip(KeyTipInput::AltTap);
        let _ = app.on_keytip(KeyTipInput::Open { refocus: false });
        assert!(!app.ribbon.keytips_active());

        // With a drawing they open.
        app.automation_op(r#"{"op":"new"}"#);
        assert!(!app.tabs[app.active_tab].is_start);
        let _ = app.on_keytip(KeyTipInput::Open { refocus: false });
        let opened = app.ribbon.keytips_active();
        // Key tips are process-wide: close them before asserting, or other
        // tests' keys go to them.
        let _ = app.on_keytip(KeyTipInput::Hide);
        assert!(opened);
        assert!(!app.ribbon.keytips_active());
    }
}
