// Module system — CadModule, ToolDef, RibbonGroup.
//
// To add a **core** ribbon tab (Home, View, …):
//   1. Create `src/modules/my_name/` directory (no `plugin.toml`)
//   2. Add `src/modules/my_name/mod.rs` implementing `CadModule` as `MyNameModule`
//   3. Add `pub mod my_name;` below
//   4. Add a `Box::new(my_name::MyNameModule)` line in `registry::all_modules()`
//
// To add an **add-on plugin** (Storm Sewer, …):
//   See `docs/plugin-architecture.md` and copy `docs/plugin-template/`.
//
// Each module folder contains:
//   - mod.rs       : module definition (ribbon groups + tool layout)
//   - <tool>.rs    : one file per tool (ribbon def + future command logic)

// ── Ribbon vocabulary (CadModule, ToolDef, RibbonGroup, …) ─────────────────
//
// These types moved to the dependency-free `ocs_plugin_api` crate so add-ons
// can target a semver-stable contract. Re-exported here to keep the long-used
// `crate::modules::{CadModule, ToolDef, …}` paths stable across the codebase.
pub use ocs_plugin_api::ribbon::{
    CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, StyleKey, ToolDef,
};

// Built-in ribbon presentation lives in the command catalog. Empty values are
// intentional sentinels: external plugins still provide their own labels and
// icons through the public ToolDef API.
const CATALOG_LABEL: &str = "";
const CATALOG_ICON: IconKind = IconKind::Glyph("");

pub(crate) fn ribbon_command(command: &'static str) -> ToolDef {
    ribbon_command_as(command, command)
}

pub(crate) fn ribbon_command_as(id: &'static str, command: &'static str) -> ToolDef {
    ToolDef {
        id,
        label: CATALOG_LABEL,
        icon: CATALOG_ICON,
        event: ModuleEvent::Command(command.to_string()),
    }
}

pub(crate) fn ribbon_action(id: &'static str, event: ModuleEvent) -> ToolDef {
    ToolDef {
        id,
        label: CATALOG_LABEL,
        icon: CATALOG_ICON,
        event,
    }
}

pub(crate) const fn ribbon_command_item(
    command: &'static str,
) -> (&'static str, &'static str, IconKind) {
    (command, CATALOG_LABEL, CATALOG_ICON)
}

pub(crate) fn ribbon_command_items(
    commands: &[&'static str],
) -> Vec<(&'static str, &'static str, IconKind)> {
    commands
        .iter()
        .copied()
        .map(ribbon_command_item)
        .collect()
}

// ── Module declarations ───────────────────────────────────────────────────

pub mod annotate;
pub mod draw;
pub mod insert;
pub mod parametric;
pub mod model;
pub mod layout;
pub mod manage;
pub mod view;

// ── Core module registry ─────────────────────────────────────────────────
// Hand-written `all_modules()` listing the built-in ribbon tabs.
pub mod registry;
