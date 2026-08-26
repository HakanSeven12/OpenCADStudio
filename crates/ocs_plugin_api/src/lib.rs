//! # Open CAD Studio plugin API
//!
//! The stable, semver-versioned contract an add-on package targets instead of
//! the `OpenCADStudio` binary internals. It is intentionally **dependency
//! free** (no `iced`, no `acadrust`) so engine crates and external tooling can
//! depend on it cheaply.
//!
//! Two pieces live here:
//!
//! - [`manifest`] — plugin identity ([`PluginManifest`]) and the host ABI
//!   version handshake ([`ApiVersion`]).
//! - [`ribbon`] — the [`CadModule`] trait and the plain-data ribbon types
//!   ([`RibbonGroup`], [`ToolDef`], …) a plugin uses to describe its tab.
//!
//! The runtime host surface a plugin uses at *dispatch* time (document access,
//! command line, undo) is `acadrust`-typed and therefore lives in the `host`
//! feature; it re-exports `acadrust` so plugins do not need to depend on it
//! directly and risk an ABI mismatch from a different version.
//!
//! For binary compatibility, the host and every plugin must resolve the same
//! `acadrust` source. The host does this via a `[patch.crates-io]` entry in
//! `Cargo.toml`; out-of-tree plugins should copy that exact patch.
//!
//! For internal architecture (process model, wire protocols, versioning policy,
//! failure modes), see `ARCHITECTURE.md` in the crate root. The modules enabled
//! by the `host` feature are:
//!
//! - `host` — plugin/runtime traits and notification types.
//! - `host_v4` — per-tab shared-memory snapshot manager.
//! - `ipc` — transport, V2/V3 protocol, and V4 multiplexed protocol.
//! - `process` — `PluginProcess` and `PluginManager`.
//! - `runner` — child-process runner entry point.
//! - `shm` — shared-memory document views.

pub mod manifest;
pub mod ribbon;
pub mod type_registry;
pub mod type_registry_types;
pub mod version_info;

/// Runtime host surface — only built with the `host` feature (pulls `acadrust`).
#[cfg(feature = "host")]
pub mod host;

/// Host-side V4 snapshot manager — only built with the `host` feature.
#[cfg(feature = "host")]
pub mod host_v4;

/// Out-of-process plugin runtime — only built with the `host` feature.
#[cfg(feature = "host")]
pub mod ipc;

/// Process management for out-of-process plugins — only built with the `host`
/// feature.
#[cfg(feature = "host")]
pub mod process;

/// Shared-memory document view — only built with the `host` feature.
#[cfg(feature = "host")]
pub mod shm;

/// Plugin runner implementation used by the host when it spawns itself in
/// runner mode — only built with the `host` feature.
#[cfg(feature = "host")]
pub mod runner;

pub use manifest::{
    effective_max_api_version, host_accepts_plugin_version, ApiVersion, PluginManifest,
    API_VERSION, API_VERSION_MIN_SUPPORTED, MAX_API_VERSION_ENV,
};
pub use ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, StyleKey, ToolDef};
pub use type_registry::{
    get_embedded_type_registry_json, EnumVariantInfo, FieldInfo, MethodInfo, ParameterInfo,
    TypeId, TypeInfo, TypeKind, TypeRegistry,
};
pub use version_info::get_embedded_version_info_json;

#[cfg(test)]
pub(crate) mod test_lock {
    //! Shared lock for tests that mutate process environment variables.
    //!
    //! Environment variables are global mutable state. Any test that sets or
    //! removes an env var must hold this lock for the duration of the mutation
    //! so tests in other modules do not observe half-written state.
    pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}

#[cfg(feature = "host")]
pub use process::{DispatchResult, PluginError, PluginManager, PluginProcess};
