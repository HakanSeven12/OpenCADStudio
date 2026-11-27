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

use std::cell::{Cell, RefCell};

use ocs_plugin_api::host::HostApi;

thread_local! {
    static CURRENT_HOST: Cell<Option<*mut dyn HostApi>> = const { Cell::new(None) };
    static UNDO_LABEL: Cell<&'static str> = const { Cell::new("") };
    static UNDO_STARTED: Cell<bool> = const { Cell::new(false) };
    static SCRIPT: RefCell<Option<(String, Option<u64>)>> = const { RefCell::new(None) };
}

/// Names the `PY_RUN` file being run, and the drag token it is resumed
/// with, for the lifetime of the guard.
pub(crate) struct ScriptGuard;

impl ScriptGuard {
    pub(crate) fn set(path: &str, resume: Option<u64>) -> Self {
        SCRIPT.with(|slot| *slot.borrow_mut() = Some((path.to_owned(), resume)));
        ScriptGuard
    }
}

impl Drop for ScriptGuard {
    fn drop(&mut self) {
        SCRIPT.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Path of the running `PY_RUN` file; `None` for `PY_EVAL` and quad code.
#[allow(dead_code)]
pub(crate) fn script_path() -> Option<String> {
    SCRIPT.with(|slot| slot.borrow().as_ref().map(|(path, _)| path.clone()))
}

/// Token of the drag this run continues, if a drag started it.
#[cfg(feature = "experimental-host-model")]
pub(crate) fn resume_token() -> Option<u64> {
    SCRIPT.with(|slot| slot.borrow().as_ref().and_then(|(_, token)| *token))
}

/// True when a `HostApi` is currently installed (and not borrowed inside `with_host`).
pub(crate) fn is_active() -> bool {
    CURRENT_HOST.with(|cell| cell.get().is_some())
}

/// Holds the thread-local host pointer for its lifetime; restores the previous
/// guard state on drop so nesting stays well-formed.
pub(crate) struct HostGuard {
    prev_host: Option<*mut dyn HostApi>,
    prev_undo_label: &'static str,
    prev_undo_started: bool,
}

impl HostGuard {
    /// `undo_label` is the label a later `ensure_undo_started()` call (from
    /// an `ocs` write function) will pass to `host.push_undo` — see its doc
    /// comment for why this is one group per script, not one per call.
    pub(crate) fn set(host: &mut dyn HostApi, undo_label: &'static str) -> Self {
        // SAFETY: erases `host`'s borrow lifetime so the fat pointer fits in a
        // `'static` thread_local slot (same layout either way: two words,
        // data + vtable). Sound only because `Drop` restores the previous slot
        // before this borrow's real lifetime ends — see `with_host`'s safety note.
        let ptr: *mut dyn HostApi =
            unsafe { std::mem::transmute::<&mut dyn HostApi, *mut dyn HostApi>(host) };
        let prev_host = CURRENT_HOST.with(|cell| cell.replace(Some(ptr)));
        let prev_undo_label = UNDO_LABEL.with(|cell| cell.replace(undo_label));
        let prev_undo_started = UNDO_STARTED.with(|cell| cell.replace(false));
        HostGuard {
            prev_host,
            prev_undo_label,
            prev_undo_started,
        }
    }
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        CURRENT_HOST.with(|cell| cell.set(self.prev_host));
        UNDO_LABEL.with(|cell| cell.set(self.prev_undo_label));
        UNDO_STARTED.with(|cell| cell.set(self.prev_undo_started));
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

/// Call before a script starts driving a command. While a plugin request
/// runs, a command joins the plugin's open undo scope, so without this the
/// script's earlier writes and the command would undo as one step. A fresh
/// scope takes the command instead, and the next write opens a new group.
pub(crate) fn split_undo_group(host: &mut dyn HostApi) {
    UNDO_STARTED.with(|started| {
        if started.replace(false) {
            host.push_undo(UNDO_LABEL.with(|label| label.get()));
        }
    });
}

struct HostRestore(Option<*mut dyn HostApi>);

impl Drop for HostRestore {
    fn drop(&mut self) {
        if let Some(ptr) = self.0.take() {
            CURRENT_HOST.with(|cell| cell.set(Some(ptr)));
        }
    }
}

/// Run `f` with the `HostApi` active for the current `PY_EVAL` call, if any.
/// Returns `None` outside of a `HostGuard`-covered call or if called reentrantly
/// while an outer `with_host` borrow is already active.
pub(crate) fn with_host<R>(f: impl FnOnce(&mut dyn HostApi) -> R) -> Option<R> {
    let ptr = CURRENT_HOST.with(|cell| cell.take())?;
    let _restore = HostRestore(Some(ptr));
    // SAFETY: `ptr` was installed by `HostGuard::set` from a live
    // `&mut dyn HostApi` whose borrow spans this call, and taking it out of
    // `CURRENT_HOST` guarantees no reentrant call can materialize a second
    // aliasing `&mut dyn HostApi`; `_restore` puts it back on return or unwind.
    Some(f(unsafe { &mut *ptr }))
}

#[cfg(test)]
pub(crate) struct StubHost {
    pub tab: usize,
    pub doc: ocs_plugin_api::host::document::Document,
    pub doc_path: Option<std::path::PathBuf>,
    pub undo_labels: Vec<String>,
}

#[cfg(test)]
impl StubHost {
    pub(crate) fn new(tab: usize) -> Self {
        Self {
            tab,
            doc: ocs_plugin_api::host::document::Document::new(),
            doc_path: None,
            undo_labels: Vec::new(),
        }
    }
}

#[cfg(test)]
impl HostApi for StubHost {
    fn tab_index(&self) -> usize {
        self.tab
    }
    fn document(&self) -> &ocs_plugin_api::host::document::Document {
        &self.doc
    }
    fn document_mut(&mut self) -> &mut ocs_plugin_api::host::document::Document {
        &mut self.doc
    }
    fn add_entity(
        &mut self,
        entity: ocs_plugin_api::host::EntityType,
    ) -> ocs_plugin_api::host::Handle {
        self.doc
            .add_entity(entity)
            .unwrap_or(ocs_plugin_api::host::Handle::NULL)
    }
    fn bump_geometry(&mut self) {}
    fn read_record(
        &self,
        _handle: ocs_plugin_api::host::Handle,
        _app_name: &str,
    ) -> Option<&ocs_plugin_api::host::ExtendedDataRecord> {
        None
    }
    fn write_record(
        &mut self,
        _handle: ocs_plugin_api::host::Handle,
        _record: ocs_plugin_api::host::ExtendedDataRecord,
    ) -> bool {
        false
    }
    fn remove_record(
        &mut self,
        _handle: ocs_plugin_api::host::Handle,
        _app_name: &str,
    ) -> bool {
        false
    }
    fn push_undo(&mut self, label: &str) {
        self.undo_labels.push(label.to_owned());
    }
    fn set_dirty(&mut self) {}
    fn push_info(&mut self, _msg: &str) {}
    fn push_output(&mut self, _msg: &str) {}
    fn push_error(&mut self, _msg: &str) {}
    fn start_interactive(
        &mut self,
        _command: Box<dyn ocs_plugin_api::host::InteractiveCommand>,
    ) {
    }
    fn plugin_state_any(&self, _plugin_id: &str) -> Option<&(dyn std::any::Any + Send + Sync)> {
        None
    }
    fn plugin_state_any_mut(
        &mut self,
        _plugin_id: &str,
    ) -> Option<&mut (dyn std::any::Any + Send + Sync)> {
        None
    }
    fn ensure_plugin_state_any(
        &mut self,
        _plugin_id: &'static str,
        _init: &mut dyn FnMut() -> Box<dyn std::any::Any + Send + Sync>,
    ) -> Option<&mut (dyn std::any::Any + Send + Sync)> {
        None
    }
    fn document_reader(&self) -> Box<dyn ocs_plugin_api::host::DocumentReader + '_> {
        unimplemented!("StubHost::document_reader unused in tests")
    }
    fn document_path(&self, _tab_id: u64) -> Option<std::path::PathBuf> {
        self.doc_path.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;

    #[test]
    fn host_guard_nests_and_with_host_blocks_reentrancy_and_restores_on_panic() {
        let mut outer = StubHost::new(1);
        let mut inner = StubHost::new(2);
        assert!(!is_active());

        {
            let _g1 = HostGuard::set(&mut outer, "outer");
            assert!(is_active());
            assert_eq!(with_host(|h| h.tab_index()), Some(1));

            with_host(|h| {
                assert_eq!(h.tab_index(), 1);
                assert!(!is_active(), "slot must be cleared while inside with_host");
                assert!(
                    with_host(|_| ()).is_none(),
                    "reentrant with_host call must fail closed"
                );
            });
            assert!(is_active());

            {
                let _g2 = HostGuard::set(&mut inner, "inner");
                assert_eq!(with_host(|h| h.tab_index()), Some(2));
            }
            assert_eq!(
                with_host(|h| h.tab_index()),
                Some(1),
                "dropping inner HostGuard must restore outer host"
            );

            let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
                with_host(|_| panic!("deliberate unwind"));
            }));
            assert!(caught.is_err());
            assert!(
                is_active(),
                "HostRestore must restore slot after panic inside with_host"
            );
            assert_eq!(with_host(|h| h.tab_index()), Some(1));
        }
        assert!(!is_active());
    }
}
