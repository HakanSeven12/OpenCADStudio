//! Process-wide cache of the most recently observed selection.
//!
//! `HostApi` has no synchronous "give me the current selection" call — only
//! the best-effort `SelectionChanged` / `SelectionChangedV4` notification.
//! Naively calling `HostApi::try_recv_notification()` from inside `dispatch`
//! doesn't work: `ocs_plugin_api::runner`'s own V4 event loop
//! (`run_v4` in `runner.rs`) drains that queue itself on every iteration,
//! forwarding each notification to `BuiltinPlugin::on_notification` *before*
//! a `Dispatch` request ever reaches `dispatch()` — so by the time `dispatch`
//! runs, the queue is always empty. The only way to observe notifications is
//! to override `on_notification` and stash what's needed here; `dispatch`
//! (and the `ocs.selection()` pyfunction) then just reads this cache.
//!
//! A plain process-wide static is fine here: one plugin process hosts exactly
//! one `PythonPlugin`, so this is equivalent to an instance field, but a
//! static is far simpler to reach from the `#[pymodule]` native functions in
//! `ocs_module.rs`, which have no path back to the `BuiltinPlugin` instance.

use std::sync::Mutex;

use ocs_plugin_api::host::Handle;

static CACHE: Mutex<Vec<Handle>> = Mutex::new(Vec::new());

pub(crate) fn set(handles: Vec<Handle>) {
    if let Ok(mut cache) = CACHE.lock() {
        *cache = handles;
    }
}

pub(crate) fn get() -> Vec<Handle> {
    CACHE.lock().map(|cache| cache.clone()).unwrap_or_default()
}
