//! OpenCAD Studio plugin template v2.
//!
//! Demonstrates read/write round-trips between the plugin and the host using
//! the zero-copy `DocumentReader` API and the validated `HostApi` RPCs.

use acadrust::entities::Point;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{EntityType, Handle};
use ocs_plugin_api::export_plugin;
use ocs_plugin_api::host::{BuiltinPlugin, HostApi, ReaderEntityKind};
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

static MANIFEST: PluginManifest = PluginManifest {
    id: "com.example.plugin-template-v2",
    name: "Plugin Template v2",
    version: env!("CARGO_PKG_VERSION"),
    description: "Demonstrates read/write object round-trips with the host.",
    api_version: ApiVersion::CURRENT,
    ribbon_order: 100,
    xdata_apps: &["SURVEYMARK"],
    command_prefixes: &[
        "COUNT_SURVEY_POINTS",
        "ADD_SURVEY_POINT",
        "MARK_FIRST_SURVEY_POINT",
        "COUNT_MARKED_SURVEY_POINTS",
    ],
};

struct PluginTemplateV2;

impl BuiltinPlugin for PluginTemplateV2 {
    fn manifest(&self) -> &'static PluginManifest {
        &MANIFEST
    }

    fn ribbon(&self) -> Box<dyn CadModule> {
        Box::new(TemplateModule)
    }

    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool {
        match cmd {
            "COUNT_SURVEY_POINTS" => {
                let count = count_survey_points(host);
                host.push_info(&format!("SURVEY points: {count}"));
                true
            }
            "ADD_SURVEY_POINT" => {
                let handle = add_survey_point(host);
                host.push_info(&format!("Added SURVEY point {handle}"));
                true
            }
            "MARK_FIRST_SURVEY_POINT" => {
                if let Some(handle) = first_survey_point_handle(host) {
                    mark_point(host, handle);
                } else {
                    host.push_info("No SURVEY point to mark");
                }
                true
            }
            "COUNT_MARKED_SURVEY_POINTS" => {
                let count = count_marked_survey_points(host);
                host.push_info(&format!("Marked SURVEY points: {count}"));
                true
            }
            _ => false,
        }
    }
}

/// Read: count point entities on the SURVEY layer via the zero-copy reader.
fn count_survey_points(host: &mut dyn HostApi) -> usize {
    let reader = host.document_reader();
    let mut count = 0usize;
    reader.for_each_entity(&mut |e| {
        if e.kind == ReaderEntityKind::Point && e.layer_name.eq_ignore_ascii_case("SURVEY") {
            count += 1;
        }
    });
    count
}

/// Write: add a new point entity on the SURVEY layer through a validated RPC.
fn add_survey_point(host: &mut dyn HostApi) -> Handle {
    let mut point = Point::from_coords(0.0, 0.0, 0.0);
    point.common.layer = "SURVEY".to_string();
    host.add_entity(EntityType::Point(point))
}

/// Read: locate the first SURVEY point and return its handle.
fn first_survey_point_handle(host: &mut dyn HostApi) -> Option<Handle> {
    let reader = host.document_reader();
    let mut handle = None;
    reader.for_each_entity(&mut |e| {
        if handle.is_none()
            && e.kind == ReaderEntityKind::Point
            && e.layer_name.eq_ignore_ascii_case("SURVEY")
        {
            handle = Some(e.handle);
        }
    });
    handle
}

/// Write: attach an XDATA record to the entity, registering the APPID.
fn mark_point(host: &mut dyn HostApi, handle: Handle) {
    let mut record = ExtendedDataRecord::new("SURVEYMARK");
    record.add_value(XDataValue::Integer32(1));
    if host.write_record(handle, record) {
        host.push_info(&format!("Marked SURVEY point {handle}"));
    } else {
        host.push_error(&format!("Failed to mark SURVEY point {handle}"));
    }
}

/// Read+write round-trip: count SURVEY points that have the SURVEYMARK XDATA.
fn count_marked_survey_points(host: &mut dyn HostApi) -> usize {
    let reader = host.document_reader();
    let mut handles = Vec::new();
    reader.for_each_entity(&mut |e| {
        if e.kind == ReaderEntityKind::Point && e.layer_name.eq_ignore_ascii_case("SURVEY") {
            handles.push(e.handle);
        }
    });
    handles
        .into_iter()
        .filter(|h| host.read_record(*h, "SURVEYMARK").is_some())
        .count()
}

struct TemplateModule;

impl CadModule for TemplateModule {
    fn id(&self) -> &'static str {
        MANIFEST.id
    }

    fn title(&self) -> &'static str {
        "Template v2"
    }

    fn ribbon_groups(&self) -> Vec<RibbonGroup> {
        vec![RibbonGroup {
            title: "Survey",
            tools: vec![
                RibbonItem::LargeTool(ToolDef {
                    id: "COUNT_SURVEY_POINTS",
                    label: "Count",
                    icon: IconKind::Glyph("C"),
                    event: ModuleEvent::Command("COUNT_SURVEY_POINTS".to_string()),
                }),
                RibbonItem::LargeTool(ToolDef {
                    id: "ADD_SURVEY_POINT",
                    label: "Add Point",
                    icon: IconKind::Glyph("+"),
                    event: ModuleEvent::Command("ADD_SURVEY_POINT".to_string()),
                }),
                RibbonItem::LargeTool(ToolDef {
                    id: "MARK_FIRST_SURVEY_POINT",
                    label: "Mark First",
                    icon: IconKind::Glyph("M"),
                    event: ModuleEvent::Command("MARK_FIRST_SURVEY_POINT".to_string()),
                }),
                RibbonItem::LargeTool(ToolDef {
                    id: "COUNT_MARKED_SURVEY_POINTS",
                    label: "Marked",
                    icon: IconKind::Glyph("*"),
                    event: ModuleEvent::Command("COUNT_MARKED_SURVEY_POINTS".to_string()),
                }),
            ],
        }]
    }
}

export_plugin!(PluginTemplateV2);
