//! A panic while a window is drawn must not end the session. On macOS the
//! unwind reaches a winit callback that cannot unwind, and the app aborts
//! with the work since the last autosave. The guard catches the panic, keeps
//! its text for the command line, and does not draw the same data again.

use std::cell::RefCell;
use std::sync::Mutex;

thread_local! {
    /// The text of the last caught draw panic that the command line has not
    /// shown. Drawing and `update` run on the same thread.
    static NOTICE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The data keys of the frames whose draw panicked. The shader widgets of
/// all viewport panes share one pipeline, so there can be more than one.
#[derive(Default)]
pub(crate) struct DrawFaults(Mutex<Vec<u64>>);

impl DrawFaults {
    /// At most this many keys are kept; the oldest goes first.
    const KEEP: usize = 8;

    pub(crate) fn has(&self, key: u64) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&key)
    }

    pub(crate) fn add(&self, key: u64) {
        let mut keys = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if !keys.contains(&key) {
            if keys.len() == Self::KEEP {
                keys.remove(0);
            }
            keys.push(key);
        }
    }
}

/// Runs `work`. Returns true when it panicked; the panic text is then kept
/// for [`take_notice`].
pub(crate) fn panicked(work: impl FnOnce()) -> bool {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(()) => false,
        Err(payload) => {
            note_panic(payload.as_ref());
            true
        }
    }
}

/// Keeps the text of a caught draw panic for [`take_notice`].
pub(crate) fn note_panic(payload: &(dyn std::any::Any + Send)) {
    let text = panic_text(payload);
    NOTICE.with(|notice| *notice.borrow_mut() = Some(text));
}

/// The text of the last caught draw panic, one time.
pub(crate) fn take_notice() -> Option<String> {
    NOTICE.with(|notice| notice.borrow_mut().take())
}

/// The text of a panic payload.
pub(crate) fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_string())
}
