//! A panic in a message handler must not end the session and take the
//! user's unsaved work with it. `update_guarded` runs the handler under a
//! panic guard: the command in progress is cancelled, what it changed is
//! taken back through the undo history, and the command line says so. The
//! crash report of the panic hook is still written.
//!
//! The guard cannot make the state after a panic correct in every case: it
//! restores the drawing, not each cache or window. So it gives up after a
//! few panics, and the next one ends the app as before.

use iced::Task;

use crate::app::{Message, OpenCADStudio};

/// After this many caught panics the next one is not caught.
pub(in crate::app) const CAUGHT_PANIC_LIMIT: u32 = 5;

/// A place where a test can make the handler panic.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum TestPanicAt {
    /// Before the handler runs.
    BeforeHandler,
    /// In `finish_all_pending_history`: the command changed the drawing and
    /// its undo record is still open.
    BeforeMessageEnd,
    /// After the handler returns, with its undo record complete.
    AfterHandler,
}

#[cfg(test)]
thread_local! {
    pub(in crate::app) static TEST_PANIC: std::cell::Cell<Option<TestPanicAt>> =
        const { std::cell::Cell::new(None) };
}

/// Panics one time if a test asked for a panic at `at`.
#[cfg(test)]
pub(in crate::app) fn test_panic_point(at: TestPanicAt) {
    if TEST_PANIC.with(|point| point.get()) == Some(at) {
        TEST_PANIC.with(|point| point.set(None));
        panic!("test panic at {at:?}");
    }
}

/// The text of a panic payload.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_string())
}

impl OpenCADStudio {
    /// [`Self::update`] under a panic guard. See the module comment.
    pub(in crate::app) fn update_guarded(&mut self, msg: Message) -> Task<Message> {
        let before = self
            .tabs
            .get(self.active_tab)
            .map(|tab| (tab.id, tab.history.undo_stack.len()));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            #[cfg(test)]
            test_panic_point(TestPanicAt::BeforeHandler);
            let task = self.update(msg);
            #[cfg(test)]
            test_panic_point(TestPanicAt::AfterHandler);
            task
        }));
        let payload = match outcome {
            Ok(task) => return task,
            Err(payload) => payload,
        };
        if self.caught_panics >= CAUGHT_PANIC_LIMIT {
            std::panic::resume_unwind(payload);
        }
        self.caught_panics += 1;
        // A panic in the recovery must not come back here as a new one.
        let undone = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.take_back_failed_command(before)
        }));
        let text = panic_text(payload.as_ref());
        let notice = match undone {
            Ok(0) => crate::tf!(
                "Internal error: {}. The command was cancelled. Save your work under a new name.",
                text
            ),
            Ok(_) => crate::tf!(
                "Internal error: {}. The command was cancelled and its changes were undone. Save your work under a new name.",
                text
            ),
            Err(_) => crate::tf!(
                "Internal error: {}. The command could not be undone. Save your work under a new name and start the application again.",
                text
            ),
        };
        self.command_line.push_error(notice.as_ref());
        Task::none()
    }

    /// Cancels the command in progress and takes back what was recorded
    /// after `before` (the active tab and the depth of its undo stack when
    /// the handler started). Returns the number of undo records taken back.
    fn take_back_failed_command(&mut self, before: Option<(u64, usize)>) -> usize {
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_some() {
            let _ = self.apply_cmd_result(crate::command::CmdResult::Cancel);
            self.tabs[i].active_cmd = None;
        }
        match before {
            // The handler can switch or close tabs; undo acts on the active
            // one, so only take back what belongs to the same tab.
            Some((id, depth)) if self.tabs[i].id == id => self.roll_back_to_undo_depth(depth),
            _ => {
                self.finish_all_pending_history();
                0
            }
        }
    }
}
