//! A plugin command that runs beside the window (#3650, step 2).
//!
//! Step 1 (`plugin_frame`) draws one frame before a plugin command; the
//! command then held the UI thread until it returned. Now, once its frame is
//! drawn, the command is dispatched without waiting: the plugin works in its
//! own process, and whatever it sends — a host request, its answer, a
//! progress or output notification — wakes the window, which serves the
//! requests here on the UI thread, one at a time, and draws in between. The
//! document stays owned by the UI thread, so nothing in the host API needs a
//! lock.
//!
//! While it runs, the status bar shows the command with the progress and
//! the latest output line the plugin reports, and a cancel button (or Esc)
//! sends the plugin `HostNotification::Cancel`, which it reads with
//! `try_recv_notification`. New commands are refused until it ends; scripts
//! wait for it, and the lines of a plugin script after it run once it ends.
//!
//! It runs this way only where waiting is not required: after its frame, with
//! a window, outside a drive to the end (`drive_headless_task`), for a command
//! exactly one loaded plugin advertises, over a V4 connection. Everything else
//! is dispatched the waiting way, as before.

use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, Mutex};

use iced::Task;
#[cfg(not(target_arch = "wasm32"))]
use ocs_plugin_api::process::PluginProcess;

use super::{Message, OpenCADStudio};

/// How often a running command is looked at without a wake: its deadline
/// is checked then.
pub(super) const CHECK_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

/// A plugin command running beside the window.
// The web build never starts a run, so it reads none of this.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) struct PluginRun {
    /// The command word, as the command line and the status bar name it.
    pub(crate) label: String,
    /// The whole line dispatched.
    line: String,
    plugin_id: String,
    /// `None` only in tests, which have no plugin process. The web build
    /// runs no plugin processes, so a run never starts there.
    #[cfg(not(target_arch = "wasm32"))]
    process: Option<Arc<PluginProcess>>,
    /// The drawing it works on.
    tab_id: u64,
    /// What the plugin reported last.
    pub(crate) percent: Option<u8>,
    pub(crate) status_line: Option<String>,
    pub(crate) cancel_requested: bool,
    /// An interactive command the plugin started meanwhile (command id).
    interactive: Option<u64>,
    /// Lines of a plugin script that come after this command.
    then: Option<(VecDeque<String>, bool)>,
}

/// The command word of a line.
fn verb(line: &str) -> &str {
    line.split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_start_matches(['_', '.'])
}

impl OpenCADStudio {
    /// Start `line` on its plugin without waiting, if it can run that way
    /// (see the module). `frame_drawn`: the command got its frame. `Some`:
    /// it started; the task delivers the plugin's wakes.
    pub(super) fn begin_plugin_run(
        &mut self,
        tab: usize,
        line: &str,
        frame_drawn: bool,
    ) -> Option<Task<Message>> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (tab, line, frame_drawn);
            None
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !frame_drawn
                || self.plugin_frame_pass
                || self.main_window.is_none()
                || self.plugin_run.is_some()
                || line.trim_start().starts_with('(')
            {
                return None;
            }
            let disabled = self.disabled_plugin_ids();
            let process = crate::plugin::external::with_manager(|manager| {
                manager.detached_dispatch_target(verb(line), |id| disabled.contains(id))
            })?;
            let (sender, wakes) = iced::futures::channel::mpsc::unbounded::<()>();
            let sender = Mutex::new(sender);
            let waker: ocs_plugin_api::ipc::v4::server::DispatchWaker = Arc::new(move || {
                let _ = sender
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .unbounded_send(());
            });
            // A report left from an earlier command is not this one's.
            let _ = crate::plugin::v4_support::take_command_status(process.id());
            match process.begin_dispatch(line, waker) {
                Ok(Some(_)) => {}
                // No V4 connection after all, or it failed to start: the
                // waiting dispatch handles and reports it.
                Ok(None) | Err(_) => return None,
            }
            self.plugin_run = Some(PluginRun {
                label: verb(line).to_ascii_uppercase(),
                line: line.to_string(),
                plugin_id: process.id().to_string(),
                process: Some(process),
                tab_id: self.tabs[tab].id,
                percent: None,
                status_line: None,
                cancel_requested: false,
                interactive: None,
                then: None,
            });
            Some(Task::run(wakes, |()| Message::PluginRunWake))
        }
    }

    /// Whether a plugin command runs beside the window.
    pub(crate) fn plugin_running(&self) -> bool {
        self.plugin_run.is_some()
    }

    /// A user command while a plugin command runs: refused, with a word on
    /// why. Requests of the running plugin itself (served in `plugin_serving`)
    /// and navigation that runs transparently pass.
    pub(super) fn plugin_run_refuses(&mut self, line: &str) -> bool {
        let Some(run) = self.plugin_run.as_ref() else {
            return false;
        };
        if self.plugin_serving {
            return false;
        }
        let word = verb(line).to_ascii_uppercase();
        if matches!(
            word.trim_start_matches('\''),
            "PAN" | "ZOOM" | "REDRAW" | "REGEN"
        ) {
            return false;
        }
        let refused = crate::i18n::plain(crate::tr!(
            "plugin-busy",
            "refused",
            command = run.label.clone()
        ));
        self.command_line.push_error(&refused);
        true
    }

    /// The drawing `tab_id` is the one the running plugin command works on:
    /// it is not closed meanwhile (said once on the command line).
    pub(super) fn plugin_run_holds_tab(&mut self, tab_id: u64) -> bool {
        if self
            .plugin_run
            .as_ref()
            .is_none_or(|run| run.tab_id != tab_id)
        {
            return false;
        }
        self.plugin_run_refuses("CLOSE")
    }

    /// The lines of a plugin script after the command that started running:
    /// they run once it ends.
    pub(super) fn plugin_run_then(&mut self, lines: VecDeque<String>, restricted: bool) {
        if let Some(run) = self.plugin_run.as_mut() {
            run.then = Some((lines, restricted));
        }
    }

    /// Ask the running plugin command to stop (cancel button, Esc).
    pub(crate) fn cancel_plugin_run(&mut self) {
        let Some(run) = self.plugin_run.as_mut() else {
            return;
        };
        if run.cancel_requested {
            return;
        }
        run.cancel_requested = true;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(process) = run.process.as_ref() {
            let _ = process.notify_plugin(None, ocs_plugin_api::host::HostNotification::Cancel);
        }
        let label = run.label.clone();
        self.command_line.push_info(&crate::i18n::plain(crate::tr!(
            "plugin-busy",
            "cancelling",
            command = label
        )));
    }

    /// The plugin sent something (or the check interval passed): serve its
    /// requests, take its report, and end the run once it answered.
    pub(super) fn on_plugin_run_wake(&mut self) -> Task<Message> {
        #[cfg(target_arch = "wasm32")]
        {
            Task::none()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(run) = self.plugin_run.as_mut() else {
                return Task::none();
            };
            if let Some(status) = crate::plugin::v4_support::take_command_status(&run.plugin_id) {
                if status.percent.is_some() {
                    run.percent = status.percent;
                }
                if status.line.is_some() {
                    run.status_line = status.line;
                }
            }
            let Some(process) = run.process.clone() else {
                return Task::none();
            };
            let tab_id = run.tab_id;
            let tab = self
                .tabs
                .iter()
                .position(|t| t.id == tab_id)
                .unwrap_or(self.active_tab);
            let mut interactive = None;
            // Served like the requests of a waiting dispatch: commands they
            // run go to the end where they are, and are not refused.
            let pass = std::mem::replace(&mut self.plugin_frame_pass, true);
            let serving = std::mem::replace(&mut self.plugin_serving, true);
            let answer = {
                let mut host = crate::plugin::host::HostSession::new(self, tab);
                process.poll_dispatch(&mut host, &mut |id| interactive = Some(id))
            };
            self.plugin_frame_pass = pass;
            self.plugin_serving = serving;
            if let (Some(id), Some(run)) = (interactive, self.plugin_run.as_mut()) {
                run.interactive = Some(id);
            }
            match answer {
                Ok(None) => Task::none(),
                Ok(Some(handled)) => self.finish_plugin_run(tab, Some(handled)),
                Err(error) => {
                    let id = process.id().to_string();
                    self.push_plugin_error(&format!("Plugin '{id}' dispatch error: {error}"));
                    self.finish_plugin_run(tab, None)
                }
            }
        }
    }

    /// The plugin answered (`Some(handled)`) or failed (`None`): what the
    /// waiting dispatch does after it returns, then what waited for it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn finish_plugin_run(&mut self, tab: usize, handled: Option<bool>) -> Task<Message> {
        let Some(run) = self.plugin_run.take() else {
            return Task::none();
        };
        if let (Some(command_id), Some(process)) = (run.interactive, run.process.as_ref()) {
            self.set_active_command(
                tab,
                Box::new(
                    crate::app::plugin_host::PluginProcessInteractiveAdapter::new(
                        Arc::clone(process),
                        command_id,
                    ),
                ),
            );
        }
        let task = match handled {
            Some(true) => {
                if self.tabs[tab].active_cmd.is_none() {
                    self.ribbon.deactivate_tool();
                }
                self.finish_dispatch(&run.line)
            }
            // Not the plugin's after all: the built-in way, as a waiting
            // dispatch that went unhandled falls through to it.
            Some(false) => {
                let suppress = std::mem::replace(&mut self.suppress_plugin_dispatch, true);
                let task = self.run_command_line(&run.line);
                self.suppress_plugin_dispatch = suppress;
                task
            }
            None => Task::none(),
        };
        match run.then {
            Some((lines, restricted)) => Task::batch([task, self.run_script_lines(lines, restricted)]),
            None => task,
        }
    }
}

#[cfg(test)]
impl PluginRun {
    /// A run without a plugin process, for tests of what surrounds it.
    pub(crate) fn for_test(line: &str, tab_id: u64) -> Self {
        PluginRun {
            label: verb(line).to_ascii_uppercase(),
            line: line.to_string(),
            plugin_id: "test".into(),
            #[cfg(not(target_arch = "wasm32"))]
            process: None,
            tab_id,
            percent: None,
            status_line: None,
            cancel_requested: false,
            interactive: None,
            then: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_running(line: &str) -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab_id = app.tabs[app.active_tab].id;
        app.plugin_run = Some(PluginRun::for_test(line, tab_id));
        app
    }

    fn last_error(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .rev()
            .find(|e| matches!(e.kind, crate::ui::command_line::EntryKind::Error))
            .map(|e| e.text.clone())
            .unwrap_or_default()
    }

    #[test]
    fn new_commands_are_refused_while_a_plugin_command_runs() {
        let mut app = app_running("EX_RUN ALL");
        let entities = app.tabs[app.active_tab].scene.document.entities().count();
        let _ = app.run_command_line("CIRCLE 0,0 5");
        assert!(last_error(&app).contains("EX_RUN"), "{}", last_error(&app));
        assert_eq!(
            app.tabs[app.active_tab].scene.document.entities().count(),
            entities,
            "the refused command drew nothing"
        );
        let _ = app.dispatch_command("LINE");
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        // Navigation passes, and so do the running plugin's own requests.
        assert!(!app.plugin_run_refuses("'ZOOM E"));
        app.plugin_serving = true;
        assert!(!app.plugin_run_refuses("LINE"));
    }

    #[test]
    fn the_drawing_it_works_on_stays_open() {
        let mut app = app_running("EX_RUN");
        let tabs = app.tabs.len();
        let id = app.tabs[app.active_tab].id;
        let _ = app.update(Message::TabClose(id));
        assert_eq!(app.tabs.len(), tabs);
        assert!(app.tabs.iter().any(|t| t.id == id));
        assert!(last_error(&app).contains("EX_RUN"));
    }

    #[test]
    fn scripts_wait_and_escape_asks_the_plugin_to_stop() {
        let mut app = app_running("EX_RUN");
        assert!(app.script_busy(), "a script waits for the command");
        let _ = app.on_command_escape();
        let run = app
            .plugin_run
            .as_ref()
            .expect("still running until it answers");
        assert!(run.cancel_requested);
        let busy = app.plugin_busy().expect("the status bar shows it");
        assert_eq!(busy.command, "EX_RUN");
        assert_eq!(busy.cancel, Some(true));
    }

    #[test]
    fn the_lines_after_the_command_run_once_it_ends() {
        let mut app = app_running("EX_RUN");
        app.plugin_run_then(VecDeque::from(["CIRCLE 0,0 5".to_string()]), false);
        let before = app.tabs[app.active_tab].scene.document.entities().count();
        let tab = app.active_tab;
        let _ = app.finish_plugin_run(tab, Some(true));
        assert!(!app.plugin_running());
        assert!(!app.script_busy());
        assert_eq!(
            app.tabs[app.active_tab].scene.document.entities().count(),
            before + 1,
            "the waiting line ran"
        );
    }

    #[test]
    fn the_verb_of_a_line() {
        assert_eq!(verb("  _OC_RUN \"/a b\" FAST"), "OC_RUN");
        assert_eq!(verb(""), "");
    }

    #[test]
    fn no_run_without_a_frame_a_window_or_a_plugin() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        assert!(
            app.begin_plugin_run(tab, "EX_RUN", false).is_none(),
            "no frame drawn"
        );
        assert!(
            app.begin_plugin_run(tab, "EX_RUN", true).is_none(),
            "no window"
        );
        app.main_window = Some(iced::window::Id::unique());
        assert!(
            app.begin_plugin_run(tab, "EX_RUN", true).is_none(),
            "no plugin advertises it"
        );
        assert!(!app.plugin_running());
        assert!(
            !app.plugin_run_refuses("LINE"),
            "nothing runs, nothing refused"
        );
    }
}
