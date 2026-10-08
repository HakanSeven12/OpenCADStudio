//! Thread-local scratch slot that gives the `ocs` module's native functions
//! access to the `HostApi` for the currently running `PY_EVAL` call.
//!
//! RustPython native functions registered via `#[pymodule]` are plain free
//! functions — `add_native_module` takes a `&'static PyModuleDef`, so there is
//! no per-call closure environment to capture `host: &mut dyn HostApi` in.
//! This is the standard pattern for bridging an embedder's per-call context
//! into embedded callbacks (the same shape as Lua userdata or a C callback's
//! `void *ctx`): stash a pointer for the duration of one interpreter run, and
//! clear it before the run's borrow of `host` ends.

use std::cell::Cell;

use ocs_plugin_api::host::HostApi;

thread_local! {
    static CURRENT_HOST: Cell<Option<*mut dyn HostApi>> = const { Cell::new(None) };
    static UNDO_LABEL: Cell<&'static str> = const { Cell::new("") };
    static UNDO_STARTED: Cell<bool> = const { Cell::new(false) };
}

/// Holds the thread-local host pointer for its lifetime; clears it on drop.
/// Must not outlive the `&mut dyn HostApi` borrow it was constructed from.
pub(crate) struct HostGuard;

impl HostGuard {
    /// `undo_label` is the label a later `ensure_undo_started()` call (from
    /// an `ocs` write function) will pass to `host.push_undo` — see its doc
    /// comment for why this is one group per script, not one per call.
    pub(crate) fn set(host: &mut dyn HostApi, undo_label: &'static str) -> Self {
        // SAFETY: erases `host`'s borrow lifetime so the fat pointer fits in a
        // `'static` thread_local slot (same layout either way: two words,
        // data + vtable). Sound only because `Drop` clears the slot before
        // this borrow's real lifetime ends — see `with_host`'s safety note.
        let ptr: *mut dyn HostApi =
            unsafe { std::mem::transmute::<&mut dyn HostApi, *mut dyn HostApi>(host) };
        CURRENT_HOST.with(|cell| cell.set(Some(ptr)));
        UNDO_LABEL.with(|cell| cell.set(undo_label));
        UNDO_STARTED.with(|cell| cell.set(false));
        HostGuard
    }
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        CURRENT_HOST.with(|cell| cell.set(None));
    }
}

/// Call from an `ocs` write function (`add_line`, ...) right before it
/// mutates the document. Starts exactly one `host.push_undo` group per
/// `PY_EVAL`/`PY_RUN` call, however many entities the script adds — a script
/// that adds ten lines should undo as one step, not ten. A no-op for a
/// script that never writes, so a pure `PY_EVAL 1 + 1` doesn't clutter the
/// undo stack.
pub(crate) fn ensure_undo_started(host: &mut dyn HostApi) {
    UNDO_STARTED.with(|started| {
        if !started.get() {
            host.push_undo(UNDO_LABEL.with(|label| label.get()));
            started.set(true);
        }
    });
}

/// Run `f` with the `HostApi` active for the current `PY_EVAL` call, if any.
/// Returns `None` outside of a `HostGuard`-covered call (a script should never
/// observe this in practice, since the guard spans the whole interpreter run).
///
/// Not reentrant: `f` must not call `with_host` again itself, since that would
/// materialize two live `&mut dyn HostApi` referring to the same host at once.
/// Fine for Phase 1.2's `selection()`/`get()`, which don't call each other.
pub(crate) fn with_host<R>(f: impl FnOnce(&mut dyn HostApi) -> R) -> Option<R> {
    CURRENT_HOST.with(|cell| {
        let ptr = cell.get()?;
        // SAFETY: `ptr` is only ever set by `HostGuard::set`, which is created
        // from a live `&mut dyn HostApi` and cleared (via `Drop`) before that
        // borrow's scope ends. The interpreter run this pointer is valid for
        // happens entirely on this thread, within that same scope.
        Some(f(unsafe { &mut *ptr }))
    })
}
