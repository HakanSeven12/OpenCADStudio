//! Open CAD Studio add-on template.
//!
//! Rename the crate (`Cargo.toml`), the ids/strings below, and `plugin.toml` to
//! match. Build with `cargo build --release` and ship the resulting cdylib plus
//! `plugin.toml` as GitHub Release assets (see `.github/workflows/release.yml`).

use ocs_plugin_api::host::{BuiltinPlugin, HostApi};
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

// Keep these fields in sync with `plugin.toml`.
static MANIFEST: PluginManifest = PluginManifest {
    id: "opencad.my_plugin",
    name: "My Plugin",
    version: "0.1.0",
    description: "What this plugin does.",
    api_version: ApiVersion::CURRENT,
    ribbon_order: 50,
    xdata_apps: &[],
    command_prefixes: &["MP_"],
};

/// The ribbon tab.
struct MyModule;

impl CadModule for MyModule {
    fn id(&self) -> &'static str {
        "my_plugin"
    }
    fn title(&self) -> &'static str {
        "My Plugin"
    }
    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![RibbonGroup {
                title: "Tools",
                tools: vec![RibbonItem::LargeTool(ToolDef {
                    id: "MP_HELLO",
                    label: "Hello",
                    icon: IconKind::Glyph("★"),
                    event: ModuleEvent::Command("MP_HELLO".to_string()),
                })],
            }]
        })
    }
}

/// The plugin entry point.
struct MyPlugin;

impl BuiltinPlugin for MyPlugin {
    fn manifest(&self) -> &'static PluginManifest {
        &MANIFEST
    }
    fn ribbon(&self) -> Box<dyn CadModule> {
        Box::new(MyModule)
    }
    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool {
        match cmd {
            "MP_HELLO" => {
                host.push_info("Hello from My Plugin");
                true
            }
            _ => false,
        }
    }
}

// Emit the C-ABI symbols the host loader looks for.
ocs_plugin_api::export_plugin!(MyPlugin);
