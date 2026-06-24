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
//! command line, undo) is `acadrust`-typed and therefore lives in the host
//! binary for now; see `docs/plugin-architecture.md` (phase 1b).

pub mod manifest;
pub mod ribbon;

/// Runtime host surface — only built with the `host` feature (pulls `acadrust`).
#[cfg(feature = "host")]
pub mod host;

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
    host_accepts_plugin_version, ApiVersion, PluginManifest, API_VERSION, API_VERSION_MIN_SUPPORTED,
};
pub use ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, StyleKey, ToolDef};

#[cfg(feature = "host")]
pub use process::{DispatchResult, PluginError, PluginManager, PluginProcess};
