//! API v2 compatibility fixture.
//!
//! Mimics an old plugin compiled against `ocs_plugin_api` v2: it reports API
//! major 2 from `ocs_plugin_api_version()` and implements only the v2 surface
//! (`HostApi` methods up to `start_interactive`). The current host must still
//! be able to load and dispatch it.

use ocs_plugin_api::host::{BuiltinPlugin, HostApi};
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

static MANIFEST: PluginManifest = PluginManifest {
    id: "opencad.my_plugin",
    name: "My Plugin",
    version: env!("CARGO_PKG_VERSION"),
    description: "API v2 fixture plugin.",
    api_version: ApiVersion { major: 2 },
    ribbon_order: 60,
    xdata_apps: &[],
    command_prefixes: &["MP_"],
};

struct MyModule;

impl CadModule for MyModule {
    fn id(&self) -> &'static str {
        "my_plugin"
    }
    fn title(&self) -> &'static str {
        "My Plugin"
    }
    fn ribbon_groups(&self) -> Vec<RibbonGroup> {
        vec![RibbonGroup {
            title: "Tools",
            tools: vec![RibbonItem::LargeTool(ToolDef {
                id: "MP_HELLO",
                label: "Hello",
                icon: IconKind::Glyph("*"),
                event: ModuleEvent::Command("MP_HELLO".to_string()),
            })],
        }]
    }
}

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
                host.push_info("Hello from API v2 plugin");
                true
            }
            _ => false,
        }
    }
}

// Custom C-ABI export that reports API v2, emulating an older build of
// `ocs_plugin_api::export_plugin!`.
#[no_mangle]
pub extern "C" fn ocs_plugin_api_version() -> u32 {
    2
}

#[no_mangle]
pub extern "C" fn ocs_plugin_register() -> *mut Box<dyn BuiltinPlugin> {
    let plugin: Box<dyn BuiltinPlugin> = Box::new(MyPlugin);
    Box::into_raw(Box::new(plugin))
}
