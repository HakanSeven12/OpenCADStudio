//! OpenCAD Studio plugin template v2.
//!
//! Demonstrates read/write round-trips between the plugin and the host using
//! the zero-copy `DocumentReader` API and the validated `HostApi` RPCs.

use ocs_plugin_api::host::acadrust::entities::Point;
use ocs_plugin_api::host::{EntityType, ExtendedDataRecord, Handle, XDataValue};
use ocs_plugin_api::dialog::{QuadActionDef, QuadItemDef, QuadPanelDef};
use ocs_plugin_api::export_plugin;
use ocs_plugin_api::host::{BuiltinPlugin, HostApi, ReaderEntityKind};
use ocs_plugin_api::manifest::{ApiVersion, EntityClasses, PluginManifest};
use ocs_plugin_api::ribbon::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

static MANIFEST: PluginManifest = PluginManifest {
    id: "com.example.plugin-template-v2",
    name: "Plugin Template v2",
    version: env!("CARGO_PKG_VERSION"),
    description: "Demonstrates read/write object round-trips with the host.",
    api_version: ApiVersion::CURRENT,
    ribbon_order: 100,
    xdata_apps: &["SURVEYMARK", "QUADINSPECT"],
    command_prefixes: &[
        "COUNT_SURVEY_POINTS",
        "ADD_SURVEY_POINT",
        "MARK_FIRST_SURVEY_POINT",
        "COUNT_MARKED_SURVEY_POINTS",
    ],
};

/// Entity-class display labels (API v10): entities carrying a
/// `SURVEY-CLASS:<key>` string in their SURVEYMARK XDATA display the mapped
/// label (e.g. "Survey Point (Point)") in the host's Properties panel and
/// `display_type` — *while this plugin is loaded*. The key is the only thing
/// stored in the drawing; the label text lives here, so it can be reworded or
/// localized without rewriting drawings.
static ENTITY_CLASSES: EntityClasses = EntityClasses {
    app_id: "SURVEYMARK",
    class_prefix: "SURVEY-CLASS:",
    labels: &[("POINT", "Survey Point")],
};

/// The quad panel's custom SVG icon: raw `<svg>` bytes, 24×24 viewBox,
/// `currentColor` strokes so the icon follows the light/dark theme. It rides
/// the same wire encoding as ribbon icons and is parsed once at registration
/// (≤ 8 KiB; malformed or over-budget bytes fall back to the glyph tile).
static QUAD_ICON: &[u8] = include_bytes!("../quad_icon.svg");

struct PluginTemplateV2;

impl BuiltinPlugin for PluginTemplateV2 {
    fn manifest(&self) -> &'static PluginManifest {
        &MANIFEST
    }

    // API v15 quad surface: one panel bound to Insert entities, registered
    // once at load. The host namespaces the visible panel id as
    // `plugin:<plugin_id>:<panel_id>` and re-resolves it host-side whenever
    // the quad target changes; registering the same panel id again would
    // replace it, and the call reports success per panel.
    fn on_load(&mut self, host: &mut dyn HostApi) {
        let registered = host.register_quad_panels(MANIFEST.id, &[QuadPanelDef {
            panel_id: "insert-ext".into(),
            title: "Insert Extras".into(),
            applies_to: vec!["Insert".into()],
            items: vec![
                QuadItemDef {
                    control_id: "explode".into(),
                    label: "Explode".into(),
                    icon: IconKind::Svg(QUAD_ICON).into(),
                    action: QuadActionDef::Command("EXPLODE".into()),
                },
                QuadItemDef {
                    control_id: "inspect".into(),
                    label: "Inspect selection".into(),
                    icon: IconKind::Glyph("I").into(),
                    action: QuadActionDef::Callback,
                },
            ],
        }]);
        if registered.iter().any(|ok| !ok) {
            host.push_error("Failed to register quad panel insert-ext");
        }
    }

    // API v15 quad callbacks: `QuadItemDef::Command` items run host-side
    // instead (EXPLODE above dispatches the host command with the selection
    // aligned to the quad target), so this only answers `Callback` items.
    // The quad target's handles come in even when they were not selected.
    fn on_quad_action(
        &mut self,
        host: &mut dyn HostApi,
        panel_id: &str,
        control_id: &str,
        handles: &[Handle],
        type_keys: &[&str],
        pick_point: [f64; 3],
    ) {
        if panel_id != "insert-ext" || control_id != "inspect" {
            return;
        }
        host.push_info(&format!(
            "quad inspect: {} entity(ies) [{}] at {:.3}, {:.3}, {:.3}",
            handles.len(),
            type_keys.join(", "),
            pick_point[0],
            pick_point[1],
            pick_point[2],
        ));
        if handles.is_empty() {
            return;
        }
        // Write window: one undo step, then record what the action saw on
        // each target handle (same write pattern as `mark_point` below).
        host.push_undo("Insert Extras inspect");
        for handle in handles {
            let mut record = ExtendedDataRecord::new("QUADINSPECT");
            record.add_value(XDataValue::String(panel_id.to_string()));
            record.add_value(XDataValue::String(control_id.to_string()));
            record.add_value(XDataValue::String(type_keys.join(",")));
            if !host.write_record(*handle, record) {
                host.push_error(&format!("Failed to stamp quad inspect on {handle}"));
            }
        }
    }

    // API v10: expose the entity-class labels declared above. The default
    // (not overriding this) declares none.
    fn entity_classes(&self) -> Option<&'static EntityClasses> {
        Some(&ENTITY_CLASSES)
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
    // Class-identity key for the entity-class labels declared above: the
    // host shows marked points as "Survey Point (Point)" while this plugin is
    // loaded, and as plain "Point" when it is not. Keys are stable identity —
    // display text never goes into the drawing.
    record.add_value(XDataValue::String("SURVEY-CLASS:POINT".to_string()));
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

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
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
    })
    }
}

export_plugin!(PluginTemplateV2);
