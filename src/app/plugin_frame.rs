//! One frame before a plugin command (#3650).
//!
//! A plugin command runs on the UI thread: the window does not draw again
//! until it returns, so a command that works for seconds leaves the screen as
//! it was, and the user cannot tell it from a hang. Before a command a loaded
//! plugin advertises starts, the window therefore draws one frame with the
//! command named in the command line and a busy pill in the status bar; the
//! command runs after that frame and the pill stays on screen while it does.
//!
//! Typed, ribbon and script commands all go through here, and so do the lines
//! of a plugin's own `CommandStep::Dispatch` script. A script waits for a frame
//! only before a line that starts a plugin command; other lines keep running
//! back to back. Without a window (batch runs, tests) nothing waits, and a
//! window that draws no frame (minimized, occluded) holds a command for at
//! most `FRAME_TIMEOUT`.

use std::collections::VecDeque;

use iced::futures::channel::oneshot;
use iced::Task;

use super::{Message, OpenCADStudio};

/// How long a command waits at most for its frame: a hidden window may not
/// draw at all.
pub(super) const FRAME_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);

/// A plugin command waiting for its frame.
pub(crate) struct PluginFrame {
    /// The command, as the command line and the status bar name it.
    pub(crate) label: String,
    /// What runs once the frame is drawn, in order.
    resumes: Vec<Resume>,
    /// Resolves the task that resumes the command. The task, not the frame
    /// tick, runs the command, so that an automation request that started it
    /// waits for it like for any other task.
    drawn: Option<oneshot::Sender<()>>,
}

/// Where a deferred command continues.
pub(crate) enum Resume {
    /// A bare command (typed, ribbon): `dispatch_command`.
    Command(String),
    /// A command line with its arguments: `run_command_line_impl`.
    Line {
        line: String,
        finish: bool,
        chain: bool,
        script_restricted: bool,
    },
    /// The next row of `script_queue`.
    ScriptRow,
    /// The rest of a plugin's script, this line first (`(lines, script_restricted)`).
    Lines(VecDeque<String>, bool),
    /// A plugin file or folder dialog's answer, dispatched as typed.
    Direct(String),
}

/// The command word of a line, for the command line and the status bar.
fn label(line: &str) -> String {
    line.split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_start_matches(['_', '.'])
        .to_ascii_uppercase()
}

impl OpenCADStudio {
    /// Hold `line` for one frame if it starts an advertised plugin command and
    /// a window will draw it. `Some(task)`: the command waits, the caller
    /// returns `task`; `None`: run it now. Called where the command is handed
    /// to the plugin; after the frame the resumed command passes once.
    pub(super) fn defer_plugin_line(
        &mut self,
        line: &str,
        resume: impl FnOnce() -> Resume,
    ) -> Option<Task<Message>> {
        if std::mem::take(&mut self.plugin_frame_drawn) {
            return None;
        }
        self.defer_plugin(line, resume)
    }

    /// `defer_plugin_line` for a script row, before it is fed: the row then
    /// reaches the dispatch, which lets the resumed command pass.
    pub(super) fn defer_plugin_row(
        &mut self,
        line: &str,
        resume: impl FnOnce() -> Resume,
    ) -> Option<Task<Message>> {
        if self.plugin_frame_drawn {
            return None;
        }
        self.defer_plugin(line, resume)
    }

    fn defer_plugin(
        &mut self,
        line: &str,
        resume: impl FnOnce() -> Resume,
    ) -> Option<Task<Message>> {
        if self.plugin_frame_pass
            || self.main_window.is_none()
            || !crate::plugin::registry::advertises(self, line)
        {
            return None;
        }
        // A second command before the first frame waits with the first.
        if let Some(frame) = self.plugin_frame.as_mut() {
            frame.resumes.push(resume());
            return Some(Task::none());
        }
        let (sender, receiver) = oneshot::channel();
        self.plugin_frame = Some(PluginFrame {
            label: label(line),
            resumes: vec![resume()],
            drawn: Some(sender),
        });
        Some(Task::perform(receiver, |_| Message::PluginFrameResume))
    }

    /// A frame was drawn (or the timeout passed): let the waiting command run.
    pub(super) fn on_plugin_frame_drawn(&mut self) {
        if let Some(sender) = self.plugin_frame.as_mut().and_then(|f| f.drawn.take()) {
            let _ = sender.send(());
        }
    }

    /// Run what waited for the frame.
    pub(super) fn resume_plugin_frame(&mut self) -> Task<Message> {
        let Some(frame) = self.plugin_frame.take() else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        for resume in frame.resumes {
            self.plugin_frame_drawn = true;
            tasks.push(match resume {
                Resume::Command(cmd) => self.dispatch_command(&cmd),
                Resume::Line {
                    line,
                    finish,
                    chain,
                    script_restricted,
                } => self.run_command_line_impl(&line, finish, chain, script_restricted),
                Resume::ScriptRow => match self.script_queue.pop_front() {
                    Some(line) => {
                        self.script_running = true;
                        self.feed_script_line(&line)
                            .chain(Task::done(Message::ScriptPump))
                    }
                    None => Task::done(Message::ScriptPump),
                },
                Resume::Lines(lines, restricted) => self.run_script_lines(lines, restricted),
                Resume::Direct(line) => self.dispatch_plugin_answer(&line, true),
            });
            // A path that did not reach a plugin command leaves nothing behind.
            self.plugin_frame_drawn = false;
        }
        Task::batch(tasks)
    }

    /// The command line's pinned prompt while a plugin command waits for its
    /// frame and then runs (beside the window, or holding it).
    pub(super) fn plugin_frame_prompt(&self) -> Option<String> {
        let label = self.plugin_busy_label()?;
        Some(crate::i18n::plain(crate::tr!(
            "plugin-busy",
            "prompt",
            command = label.to_string()
        )))
    }

    /// The plugin command the status bar shows as running, with what it
    /// reported.
    pub(super) fn plugin_busy(&self) -> Option<crate::ui::statusbar::PluginBusy<'_>> {
        if let Some(frame) = self.plugin_frame.as_ref() {
            return Some(crate::ui::statusbar::PluginBusy {
                command: &frame.label,
                percent: None,
                line: None,
                cancel: None,
            });
        }
        self.plugin_run
            .as_ref()
            .map(|run| crate::ui::statusbar::PluginBusy {
                command: &run.label,
                percent: run.percent,
                line: run.status_line.as_deref(),
                cancel: Some(run.cancel_requested),
            })
    }

    /// The plugin command the status bar shows as running.
    pub(super) fn plugin_busy_label(&self) -> Option<&str> {
        self.plugin_frame
            .as_ref()
            .map(|frame| frame.label.as_str())
            .or_else(|| self.plugin_run.as_ref().map(|run| run.label.as_str()))
    }

    /// The lines of a plugin's script, each answering what the one before
    /// left open; before a line that starts a plugin command the window draws
    /// a frame, and the rest of the script waits with it.
    pub(super) fn run_script_lines(
        &mut self,
        mut lines: VecDeque<String>,
        restricted: bool,
    ) -> Task<Message> {
        while let Some(line) = lines.front().cloned() {
            let line = line.trim().to_string();
            if line.is_empty() {
                lines.pop_front();
                continue;
            }
            if restricted {
                if !self.script_commands {
                    let err = "running commands from a script is turned off (SCRIPTCOMMANDS = 0); turn it on at the command line with SCRIPTCOMMANDS 1";
                    self.script_command_rejected = Some(err.to_owned());
                    self.command_line.push_error(err);
                    break;
                }
                if let Err(err) = self.check_script_command_allowed(&line) {
                    self.script_command_rejected = Some(err.clone());
                    self.command_line.push_error(&err);
                    break;
                }
            }
            if let Some(task) =
                self.defer_plugin_row(&line, || Resume::Lines(lines.clone(), restricted))
            {
                return task;
            }
            lines.pop_front();
            // A plugin line that got its frame may run beside the window; the
            // rest of the script then waits for it to end. Any other line
            // runs to the end here, and what it starts does not wait.
            let beside = self.plugin_frame_drawn && !self.plugin_frame_pass;
            let pass = std::mem::replace(&mut self.plugin_frame_pass, !beside);
            let task = if restricted {
                self.run_script_command_line(&line, true, true)
            } else {
                self.run_command_line_streaming(&line, true)
            };
            if beside && self.plugin_running() {
                self.plugin_frame_pass = pass;
                self.plugin_frame_drawn = false;
                self.plugin_run_then(lines, restricted);
                return task;
            }
            let _ = self.drive_headless_task(task);
            self.plugin_frame_pass = pass;
            if restricted && self.script_command_rejected.take().is_some() {
                break;
            }
        }
        // A line that did not reach a plugin command leaves nothing behind.
        self.plugin_frame_drawn = false;
        Task::none()
    }

    /// A plugin file or folder dialog's answer: `"<command> <path>"`, with
    /// the path's case intact. `frame_drawn`: it waited for its frame and may
    /// run beside the window.
    pub(super) fn dispatch_plugin_answer(
        &mut self,
        line: &str,
        frame_drawn: bool,
    ) -> Task<Message> {
        if self.plugin_run_refuses(line) {
            return Task::none();
        }
        let i = self.active_tab;
        if let Some(task) = self.begin_plugin_run(i, line, frame_drawn) {
            return task;
        }
        if !crate::plugin::try_dispatch(self, i, line) {
            let command = line.split_whitespace().next().unwrap_or_default();
            self.command_line
                .push_error(crate::tf!("No plugin handled: {command}").as_ref());
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::registry::TEST_ADVERTISED;

    /// An app with a main window, so that plugin commands wait for a frame,
    /// and `verbs` advertised as plugin commands.
    fn with_window(verbs: &[&str]) -> OpenCADStudio {
        TEST_ADVERTISED.with(|t| *t.borrow_mut() = verbs.iter().map(|v| v.to_string()).collect());
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.main_window = Some(iced::window::Id::unique());
        app
    }

    fn clear() {
        TEST_ADVERTISED.with(|t| t.borrow_mut().clear());
    }

    #[test]
    fn a_plugin_command_waits_for_a_frame_and_names_itself() {
        let mut app = with_window(&["EX_RUN"]);
        let task = app.dispatch_command("ex_run");
        assert!(task.units() > 0, "the caller gets the task that resumes it");
        assert_eq!(app.plugin_busy_label(), Some("EX_RUN"));
        assert!(app
            .plugin_frame_prompt()
            .is_some_and(|p| p.contains("EX_RUN") && !p.contains(['\u{2068}', '\u{2069}'])));
        // The frame arrives: the command runs in the task's message, once.
        app.on_plugin_frame_drawn();
        assert!(app.plugin_frame.as_ref().is_some_and(|f| f.drawn.is_none()));
        let _ = app.resume_plugin_frame();
        assert!(app.plugin_frame.is_none(), "it ran and did not wait again");
        assert!(!app.plugin_frame_drawn);
        clear();
    }

    #[test]
    fn other_commands_and_headless_runs_do_not_wait() {
        let mut app = with_window(&["EX_RUN"]);
        let _ = app.dispatch_command("REGEN");
        assert!(
            app.plugin_frame.is_none(),
            "a built-in command runs at once"
        );
        let _ = app.dispatch_command("(princ)");
        assert!(app.plugin_frame.is_none(), "Lisp is never a plugin command");
        app.main_window = None;
        let _ = app.dispatch_command("EX_RUN");
        assert!(
            app.plugin_frame.is_none(),
            "no window, no frame to wait for"
        );
        clear();
    }

    #[test]
    fn a_command_line_with_arguments_waits_and_resumes_whole() {
        let mut app = with_window(&["EX_RUN"]);
        let _ = app.run_command_line("EX_RUN \"/a b/c\" FAST");
        let frame = app.plugin_frame.as_ref().expect("waits");
        assert!(matches!(
            &frame.resumes[..],
            [Resume::Line { line, .. }] if line == "EX_RUN \"/a b/c\" FAST"
        ));
        clear();
    }

    #[test]
    fn a_script_waits_only_before_plugin_lines() {
        let mut app = with_window(&["EX_RUN"]);
        app.script_queue.extend([
            "REGEN".to_string(),
            "EX_RUN".to_string(),
            "REGEN".to_string(),
        ]);
        // The first row is no plugin command: it is fed at once.
        let _ = app.pump_script();
        assert!(app.plugin_frame.is_none());
        assert_eq!(app.script_queue.len(), 2);
        // The plugin row waits, still queued, until its frame.
        let _ = app.pump_script();
        assert_eq!(app.plugin_busy_label(), Some("EX_RUN"));
        assert_eq!(app.script_queue.len(), 2);
        let _ = app.resume_plugin_frame();
        assert_eq!(app.script_queue.len(), 1, "after the frame the row runs");
        assert!(app.plugin_frame.is_none());
        clear();
    }

    #[test]
    fn a_plugin_script_runs_up_to_its_plugin_line_then_waits_with_the_rest() {
        let mut app = with_window(&["EX_NOTE", "EX_RUN"]);
        let lines: VecDeque<String> = ["REGEN", "EX_NOTE working", "EX_RUN ALL"]
            .map(str::to_string)
            .into();
        let _ = app.run_script_lines(lines, false);
        let frame = app.plugin_frame.as_ref().expect("waits before EX_NOTE");
        assert_eq!(frame.label, "EX_NOTE");
        assert!(matches!(&frame.resumes[..], [Resume::Lines(rest, _)] if rest.len() == 2));
        // After the frame EX_NOTE runs, then EX_RUN waits for one of its own.
        let _ = app.resume_plugin_frame();
        let frame = app.plugin_frame.as_ref().expect("waits before EX_RUN");
        assert_eq!(frame.label, "EX_RUN");
        assert!(matches!(&frame.resumes[..], [Resume::Lines(rest, _)] if rest.len() == 1));
        let _ = app.resume_plugin_frame();
        assert!(app.plugin_frame.is_none());
        clear();
    }

    #[test]
    fn a_plugin_command_run_to_the_end_never_waits_for_a_frame() {
        // Automation, Lisp and a plugin's script inside them make a line and
        // drive it where they are: a wait for a frame would hold the UI
        // thread for good.
        let mut app = with_window(&["EX_RUN"]);
        let task = app.run_command_line("EX_RUN ALL");
        assert!(
            app.plugin_frame.is_some(),
            "on its own it waits for a frame"
        );
        assert!(app.drive_headless_task(task).is_ok());
        assert!(app.plugin_frame.is_none(), "driven, it ran without waiting");
        assert!(!app.plugin_frame_pass, "the pass ends with the drive");
        // A line driven where it is made defers nothing at all.
        let lines: VecDeque<String> = ["EX_RUN ALL"].map(str::to_string).into();
        app.plugin_frame_pass = true;
        let _ = app.run_script_lines(lines, false);
        assert!(app.plugin_frame.is_none());
        app.plugin_frame_pass = false;
        clear();
    }

    #[test]
    fn a_second_command_before_the_frame_waits_with_the_first() {
        let mut app = with_window(&["EX_A", "EX_B"]);
        let _ = app.dispatch_command("EX_A");
        let second = app.dispatch_command("EX_B");
        assert_eq!(second.units(), 0);
        let frame = app.plugin_frame.as_ref().expect("waits");
        assert_eq!(frame.label, "EX_A");
        assert_eq!(frame.resumes.len(), 2);
        let _ = app.resume_plugin_frame();
        assert!(app.plugin_frame.is_none());
        clear();
    }
}
