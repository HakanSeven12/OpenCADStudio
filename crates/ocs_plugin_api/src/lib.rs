//! # Open CAD Studio plugin API
//!
//! The stable, semver-versioned contract an add-on package targets instead of
//! the `OpenCADStudio` binary internals. It is intentionally **dependency
//! free** (no `iced`, no `document`) so engine crates and external tooling can
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
//! command line, undo) is `document`-typed and therefore lives in the `host`
//! feature; it re-exports `document` so plugins do not need to depend on it
//! directly and risk an ABI mismatch from a different version.
//!
//! For binary compatibility, the host and every plugin must resolve the same
//! `document` source. The host does this via a `[patch.crates-io]` entry in
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

pub mod dialog;
pub mod entity_coverage;
pub mod entity_coverage_types;
pub mod manifest;
pub mod ribbon;
pub mod scene;
pub mod scene_builder;
pub mod type_registry;
pub mod type_registry_types;
pub mod version_info;
pub mod widgets;

/// Runtime host surface — only built with the `host` feature (pulls `document`).
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

pub use dialog::*;
pub use entity_coverage::{
    get_embedded_entity_coverage_json, EntityCoverageCatalog, EntityKindCoverage, EntityScope,
    ModelAccess, PropertyCoverage,
};
pub use manifest::{
    effective_max_api_version, host_accepts_plugin_version, ApiVersion, PluginManifest,
    API_VERSION, API_VERSION_MIN_SUPPORTED, MAX_API_VERSION_ENV,
};
pub use ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, StyleKey, ToolDef};
pub use scene::{
    AppliedPatchReport, AssetKind, CubicSegment, DragMeta, MarkerKind, PanelScene, PanelScenePatch,
    Rect2D, Rgba8, SceneColor, SceneElement, SceneError, SceneKindCounts, SceneOp, SceneRevision,
    SceneSummary, StableId, Stroke, ThemeColor, Vec2,
};
pub use scene_builder::{
    fixture_graph_scene, fixture_graph_scene_plan, fixture_scene_with_files, try_fixture_graph_scene,
    PatchBuilder, SceneBuilder,
};
pub use type_registry::{
    get_embedded_type_registry_json, EnumVariantInfo, FieldInfo, MethodInfo, ParameterInfo, TypeId,
    TypeInfo, TypeKind, TypeRegistry,
};
pub use version_info::get_embedded_version_info_json;
pub use widgets::{
    Axis, BadgeTone, CellKind, ConfirmPolicy, DomainRef, IconActionDef, IconRef, ItemDef,
    ItemListDef, MenuDef, MenuItemDef, PanelTableDef, TableCell, TableColumnDef, TableRef,
    TableRowDef, TreeCapabilities, TreeRowDef, WidgetCellValue, WidgetError, WidgetEvent,
    WidgetGroup, WidgetKindCounts, WidgetNode, WidgetOp, WidgetRange, WidgetSummary, WidgetSurface,
    WidgetSurfacePatch, WidgetTreeDef,
};

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
