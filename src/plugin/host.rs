// Plugin traits — HostSession lives in `app::plugin_host` (same-crate field
// access) and implements the stable `HostApi` contract plugins target.

// The session adapter the registry wraps `app` in to dispatch to plugins. The
// plugin-facing contract — `BuiltinPlugin` + `HostApi` — lives in
// `ocs_plugin_api`; external cdylibs target it directly.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use crate::app::plugin_host::HostSession;
