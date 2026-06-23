//! Runtime host surface (`host` feature).
//!
//! [`HostApi`] is the `acadrust`-typed adapter a plugin uses at *dispatch* time
//! — document access, entity creation, XDATA, undo, and the command line. It is
//! the stable counterpart to the dependency-free manifest/ribbon contract: a
//! plugin's `dispatch` receives `&mut dyn HostApi` rather than the host's
//! concrete session type, so an out-of-tree add-on compiles against this crate
//! alone.
//!
//! Per-tab plugin state is keyed by `manifest.id`. The trait exposes it in an
//! object-safe `Any` form; use the [`plugin_state`], [`plugin_state_mut`] and
//! [`ensure_plugin_state`] helpers for the ergonomic typed access.

use std::any::Any;

use acadrust::xdata::ExtendedDataRecord;
use acadrust::{CadDocument, EntityType, Handle};

use crate::manifest::PluginManifest;
use crate::ribbon::CadModule;

/// An add-on package's entry point: its manifest, optional ribbon tab, and
/// command dispatch. Built-in (in-tree) and dynamically-loaded (cdylib) plugins
/// implement the same trait from this crate, so an out-of-tree add-on targets
/// the stable contract rather than the host binary.
pub trait BuiltinPlugin: Send + Sync {
    fn manifest(&self) -> &'static PluginManifest;
    fn ribbon(&self) -> Box<dyn CadModule>;
    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool;
}

/// A point-driven interactive command a plugin starts via
/// [`HostApi::start_interactive`]. The host shows the prompt, collects points —
/// clicked in the viewport, or fed as coordinates over the `--serve` automation
/// API — and commits the entities the command yields, exactly like a built-in
/// tool. This is the plugin-facing slice of the host's command machinery; it
/// covers click-to-place placement without exposing the host's internal command
/// trait.
pub trait InteractiveCommand: Send {
    /// Prompt for the next point.
    fn prompt(&self) -> String;
    /// A point was supplied (clicked or typed `x,y[,z]`). Returns the next step.
    fn on_point(&mut self, pt: [f64; 3]) -> CommandStep;
    /// Enter pressed with no point — e.g. to finish a multi-point command.
    fn on_enter(&mut self) -> CommandStep {
        CommandStep::Cancel
    }

    /// When `true`, the next input picks an existing **entity** (the user clicks
    /// on it; over `--serve`, a handle is supplied) rather than a free point —
    /// the host then calls [`on_object_pick`](Self::on_object_pick). Use this to
    /// reference existing geometry (e.g. connect a pipe between two structures).
    fn needs_object_pick(&self) -> bool {
        false
    }
    /// An existing entity was picked: its `handle` and the pick point. Read the
    /// entity's data (XDATA / geometry) via `HostApi`, keyed by the handle.
    fn on_object_pick(&mut self, _handle: Handle, _pt: [f64; 3]) -> CommandStep {
        CommandStep::Cancel
    }
}

/// The outcome of an [`InteractiveCommand`] step.
#[derive(Debug)]
#[cfg_attr(feature = "host", derive(serde::Serialize, serde::Deserialize))]
pub enum CommandStep {
    /// Need another point; keep the command active.
    NeedPoint,
    /// Commit an entity to the document and keep collecting points.
    Commit(EntityType),
    /// Commit an entity and end the command.
    CommitAndEnd(EntityType),
    /// End the command without committing.
    Done,
    /// Cancel the command.
    Cancel,
}

/// Export a `BuiltinPlugin` from a `cdylib` so the host can load it at runtime.
///
/// Emits the two C symbols the loader looks for: `ocs_plugin_api_version`
/// (checked before anything else, so an ABI-incompatible build is rejected
/// without running its code) and `ocs_plugin_register` (constructs the plugin
/// and hands ownership to the host as a boxed trait object).
///
/// ```ignore
/// ocs_plugin_api::export_plugin!(MyPlugin::new());
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($ctor:expr) => {
        #[no_mangle]
        pub extern "C" fn ocs_plugin_api_version() -> u32 {
            $crate::API_VERSION
        }

        #[no_mangle]
        pub extern "C" fn ocs_plugin_register(
        ) -> *mut ::std::boxed::Box<dyn $crate::host::BuiltinPlugin> {
            let plugin: ::std::boxed::Box<dyn $crate::host::BuiltinPlugin> =
                ::std::boxed::Box::new($ctor);
            ::std::boxed::Box::into_raw(::std::boxed::Box::new(plugin))
        }
    };
}

/// The plugin-facing runtime surface for one active document tab.
pub trait HostApi {
    /// Index of the tab this session targets.
    fn tab_index(&self) -> usize;

    // ── Document ────────────────────────────────────────────────────────────
    fn document(&self) -> &CadDocument;
    fn document_mut(&mut self) -> &mut CadDocument;
    /// Add an entity to the active document, returning its handle.
    fn add_entity(&mut self, entity: EntityType) -> Handle;
    /// Mark the scene geometry dirty so it is re-tessellated next frame.
    fn bump_geometry(&mut self);

    // ── XDATA ───────────────────────────────────────────────────────────────
    /// Read the XDATA record for `app_name` on entity `handle`, if any.
    fn read_record(&self, handle: Handle, app_name: &str) -> Option<&ExtendedDataRecord>;
    /// Attach `record` to entity `handle`, replacing any existing record for the
    /// same application and registering the APPID. Returns `false` if the entity
    /// does not exist.
    fn write_record(&mut self, handle: Handle, record: ExtendedDataRecord) -> bool;
    /// Remove the XDATA record for `app_name` from entity `handle`. Returns
    /// `true` if a record was removed.
    fn remove_record(&mut self, handle: Handle, app_name: &str) -> bool;

    // ── Undo / dirty ────────────────────────────────────────────────────────
    fn push_undo(&mut self, label: &str);
    fn set_dirty(&mut self);

    // ── Command line ────────────────────────────────────────────────────────
    fn push_info(&mut self, msg: &str);
    fn push_output(&mut self, msg: &str);
    fn push_error(&mut self, msg: &str);

    /// Start a plugin-defined interactive (click-to-place) command on the active
    /// tab. The host drives it through its normal point-collection flow.
    fn start_interactive(&mut self, command: Box<dyn InteractiveCommand>);

    // ── Per-tab plugin state (object-safe; use the typed helpers below) ──────
    fn plugin_state_any(&self, plugin_id: &str) -> Option<&(dyn Any + Send + Sync)>;
    fn plugin_state_any_mut(&mut self, plugin_id: &str)
        -> Option<&mut (dyn Any + Send + Sync)>;
    /// Get the state for `plugin_id`, inserting `init()`'s result if absent.
    fn ensure_plugin_state_any(
        &mut self,
        plugin_id: &'static str,
        init: &mut dyn FnMut() -> Box<dyn Any + Send + Sync>,
    ) -> &mut (dyn Any + Send + Sync);
}

/// Typed read of per-tab plugin state stored under `plugin_id`.
pub fn plugin_state<'a, T: Any + Send + Sync>(
    host: &'a dyn HostApi,
    plugin_id: &str,
) -> Option<&'a T> {
    host.plugin_state_any(plugin_id)?.downcast_ref::<T>()
}

/// Typed mutable access to per-tab plugin state stored under `plugin_id`.
pub fn plugin_state_mut<'a, T: Any + Send + Sync>(
    host: &'a mut dyn HostApi,
    plugin_id: &str,
) -> Option<&'a mut T> {
    host.plugin_state_any_mut(plugin_id)?.downcast_mut::<T>()
}

/// Typed get-or-insert of per-tab plugin state stored under `plugin_id`.
pub fn ensure_plugin_state<'a, T: Any + Send + Sync>(
    host: &'a mut dyn HostApi,
    plugin_id: &'static str,
    init: impl FnOnce() -> T,
) -> &'a mut T {
    let mut init = Some(init);
    let any = host.ensure_plugin_state_any(plugin_id, &mut || {
        Box::new((init.take().expect("init called once"))())
    });
    any.downcast_mut::<T>()
        .expect("plugin state type mismatch for plugin_id")
}
