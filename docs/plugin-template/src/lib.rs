//! Open CAD Studio add-on template.
//!
//! Rename the crate (`Cargo.toml`), the ids/strings below, and `plugin.toml` to
//! match. Build with `cargo build --release` and ship the resulting cdylib plus
//! `plugin.toml` as GitHub Release assets (see `.github/workflows/release.yml`).

use std::sync::atomic::{AtomicU64, Ordering};

use acadrust::entities::Point;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use ocs_plugin_api::host::{BuiltinPlugin, HostApi, HostNotification, PluginTool};
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

/// Last `DocumentChanged` epoch this plugin observed (test hook: the
/// deterministic-boot notification test reads it through `MP_LAST_NOTIF`).
static LAST_DOC_VERSION: AtomicU64 = AtomicU64::new(u64::MAX);

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
            // Test hook: report the last observed DocumentChanged epoch so a
            // fresh-boot test can assert the notification channel attached
            // deterministically (u64::MAX means none arrived yet).
            "MP_LAST_NOTIF" => {
                let version = LAST_DOC_VERSION.load(Ordering::SeqCst);
                host.push_info(&format!("LAST_NOTIF {version}"));
                true
            }
            // Test hook (API v13.1): the FIRST-WRITE-BOUNDARY shape — a
            // dispatch whose nested write frames are served INLINE under the
            // manager borrow held by the dispatch path (the boundary publish
            // re-enters with_manager). The v13 AppHangB1 hang the consumer
            // captured on CVI_ALIGN_BYPI was exactly this call.
            "MP_WRITE" => {
                let point = Point::from_coords(1.0, 2.0, 0.0);
                let handle = host.add_entity(acadrust::EntityType::Point(point));
                if !handle.is_null() {
                    let _ = host.write_record(
                        handle,
                        ExtendedDataRecord {
                            application_name: "MYPLUG".to_string(),
                            values: vec![XDataValue::String("mp-write".to_string())],
                        },
                    );
                }
                host.push_info("MP_WRITE done");
                true
            }
            _ => false,
        }
    }

    fn on_notification(&mut self, _command_id: Option<u64>, notification: HostNotification) {
        if let HostNotification::DocumentChanged { version } = notification {
            LAST_DOC_VERSION.store(version, Ordering::SeqCst);
        }
    }

    // AI tools (API v25): typed operations an AI client calls over the
    // automation bridge.
    fn tools(&self) -> Vec<PluginTool> {
        vec![PluginTool {
            name: "add_point".to_string(),
            description: "Add a point entity at x, y (z = 0) and return its handle.".to_string(),
            input_schema: r#"{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false}"#.to_string(),
        }]
    }

    fn call_tool(
        &mut self,
        host: &mut dyn HostApi,
        name: &str,
        arguments: &str,
    ) -> Result<String, String> {
        match name {
            // The host checked `arguments` against the schema above and
            // wraps the call in one undo step.
            "add_point" => {
                let args: serde_json::Value =
                    serde_json::from_str(arguments).map_err(|e| e.to_string())?;
                let (x, y) = (args["x"].as_f64().unwrap_or(0.0), args["y"].as_f64().unwrap_or(0.0));
                let handle = host.add_entity(acadrust::EntityType::Point(Point::from_coords(x, y, 0.0)));
                if handle.is_null() {
                    return Err("the point could not be added".to_string());
                }
                Ok(serde_json::json!({ "handle": format!("{:X}", handle.value()) }).to_string())
            }
            _ => Err(format!("unknown tool '{name}'")),
        }
    }
}

// Emit the C-ABI symbols the host loader looks for.
ocs_plugin_api::export_plugin!(MyPlugin);
