use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use cargo_lock::Lockfile;
use serde::de::DeserializeOwned;
use serde_reflection::{
    ContainerFormat, Format, Named, Registry, Samples, Tracer, TracerConfig, VariantFormat,
};

// Include the stable schema types so the same definitions are used at build
// time and at runtime. The file is self-contained and only depends on serde.
include!("src/type_registry_types.rs");

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    generate_type_registry(&out_dir);
    generate_version_info(&out_dir);
    println!(
        "cargo:rerun-if-changed={}",
        workspace_cargo_lock_path().display()
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Type registry
// ════════════════════════════════════════════════════════════════════════════

fn generate_type_registry(out_dir: &Path) {
    let mut tracer = Tracer::new(TracerConfig::default());
    let mut samples = Samples::new();
    add_enum_samples(&mut tracer, &mut samples);

    // Trace the complete record roots used by the editor and MCP API. Nested
    // entity/object variants and their enums are added to the same registry.
    type TraceFn = fn(&mut Tracer, &Samples);
    let types: Vec<(&str, TraceFn)> = vec![
        ("EntityType", trace::<acadrust::EntityType>),
        ("ObjectType", trace::<acadrust::objects::ObjectType>),
        (
            "HeaderVariables",
            trace::<acadrust::document::HeaderVariables>,
        ),
        ("SummaryInfo", trace::<acadrust::document::SummaryInfo>),
        ("LineType", trace::<acadrust::LineType>),
        ("TextStyle", trace::<acadrust::TextStyle>),
        ("BlockRecord", trace::<acadrust::BlockRecord>),
        ("DimStyle", trace::<acadrust::DimStyle>),
        ("AppId", trace::<acadrust::AppId>),
        ("View", trace::<acadrust::View>),
        ("VPort", trace::<acadrust::VPort>),
        ("Ucs", trace::<acadrust::Ucs>),
        ("VxTableRecord", trace::<acadrust::VxTableRecord>),
        ("DxfClass", trace::<acadrust::classes::DxfClass>),
        (
            "BlockVisibilityParameter",
            trace::<acadrust::objects::BlockVisibilityParameter>,
        ),
        ("FieldDef", trace::<acadrust::document::FieldDef>),
        (
            "DgnLsDefinition",
            trace::<acadrust::objects::DgnLsDefinition>,
        ),
        ("DgnLsComponent", trace::<acadrust::objects::DgnLsComponent>),
        (
            "NotificationCollection",
            trace::<acadrust::notification::NotificationCollection>,
        ),
        ("Preview", trace::<acadrust::document::Preview>),
        ("Point", trace::<acadrust::Point>),
        ("Line", trace::<acadrust::Line>),
        ("Circle", trace::<acadrust::Circle>),
        ("Arc", trace::<acadrust::Arc>),
        ("Ellipse", trace::<acadrust::Ellipse>),
        ("Polyline", trace::<acadrust::Polyline>),
        ("Polyline2D", trace::<acadrust::entities::Polyline2D>),
        ("Polyline3D", trace::<acadrust::entities::Polyline3D>),
        ("LwPolyline", trace::<acadrust::LwPolyline>),
        ("MText", trace::<acadrust::entities::MText>),
        ("Spline", trace::<acadrust::Spline>),
        ("EntityCommon", trace::<acadrust::entities::EntityCommon>),
        ("Handle", trace::<acadrust::Handle>),
        ("Vector2", trace::<acadrust::Vector2>),
        ("Vector3", trace::<acadrust::Vector3>),
        ("Color", trace::<acadrust::Color>),
        ("Layer", trace::<acadrust::Layer>),
        ("XDataValue", trace::<acadrust::xdata::XDataValue>),
        ("XRecord", trace::<acadrust::objects::XRecord>),
        ("XRecordEntry", trace::<acadrust::objects::XRecordEntry>),
        ("XRecordValue", trace::<acadrust::objects::XRecordValue>),
        (
            "XRecordValueType",
            trace::<acadrust::objects::XRecordValueType>,
        ),
        ("XRecordSection", trace::<acadrust::objects::XRecordSection>),
        (
            "DictionaryCloningFlags",
            trace::<acadrust::objects::DictionaryCloningFlags>,
        ),
        (
            "KnownXRecordKind",
            trace::<acadrust::objects::KnownXRecordKind>,
        ),
        (
            "ProxyObjectReference",
            trace::<acadrust::objects::ProxyObjectReference>,
        ),
        (
            "ProxyReferenceKind",
            trace::<acadrust::objects::ProxyReferenceKind>,
        ),
    ];

    for (name, f) in types {
        f(&mut tracer, &samples);
        // serde-reflection accumulates named types as it traces; we only need
        // to ensure the seed types are recorded even if a nested trace fails.
        eprintln!(
            "[ocs_plugin_api build] traced type registry entry: {}",
            name
        );
    }

    type TraceSimpleFn = fn(&mut Tracer);
    let enums: Vec<(&str, TraceSimpleFn)> = vec![
        (
            "AssocAnnotationKind",
            trace_simple::<acadrust::objects::AssocAnnotationKind>,
        ),
        (
            "AssocConstraintNodeData",
            trace_simple::<acadrust::objects::AssocConstraintNodeData>,
        ),
        (
            "AssocEvalValue",
            trace_simple::<acadrust::objects::AssocEvalValue>,
        ),
        (
            "AssocSubcurveKind",
            trace_simple::<acadrust::objects::AssocSubcurveKind>,
        ),
        (
            "AssocSurfaceActionKind",
            trace_simple::<acadrust::objects::AssocSurfaceActionKind>,
        ),
        (
            "AssocViewObjectActionParamKind",
            trace_simple::<acadrust::objects::AssocViewObjectActionParamKind>,
        ),
        (
            "AcisVersion",
            trace_simple::<acadrust::entities::AcisVersion>,
        ),
        (
            "AssociativeData",
            trace_simple::<acadrust::objects::AssociativeData>,
        ),
        (
            "AttachmentPointType",
            trace_simple::<acadrust::entities::AttachmentPointType>,
        ),
        (
            "BlockContentConnectionType",
            trace_simple::<acadrust::entities::BlockContentConnectionType>,
        ),
        (
            "BlockEvalValue",
            trace_simple::<acadrust::objects::BlockEvalValue>,
        ),
        ("BorderType", trace_simple::<acadrust::entities::BorderType>),
        (
            "BoundaryEdge",
            trace_simple::<acadrust::entities::BoundaryEdge>,
        ),
        (
            "BreakFlowDirection",
            trace_simple::<acadrust::entities::BreakFlowDirection>,
        ),
        (
            "CellAlignment",
            trace_simple::<acadrust::objects::CellAlignment>,
        ),
        (
            "CellStyleType",
            trace_simple::<acadrust::entities::CellStyleType>,
        ),
        ("CellType", trace_simple::<acadrust::entities::CellType>),
        (
            "CellValueType",
            trace_simple::<acadrust::entities::CellValueType>,
        ),
        (
            "ClassObjectData",
            trace_simple::<acadrust::objects::ClassObjectData>,
        ),
        ("ClipMode", trace_simple::<acadrust::entities::ClipMode>),
        ("ClipType", trace_simple::<acadrust::entities::ClipType>),
        (
            "CompoundEntry",
            trace_simple::<acadrust::compound_file::CompoundEntry>,
        ),
        (
            "CompoundPropertyValue",
            trace_simple::<acadrust::compound_file::CompoundPropertyValue>,
        ),
        (
            "CompoundStreamContent",
            trace_simple::<acadrust::compound_file::CompoundStreamContent>,
        ),
        (
            "DataObjectData",
            trace_simple::<acadrust::objects::DataObjectData>,
        ),
        (
            "DgnLineStyleData",
            trace_simple::<acadrust::objects::DgnLineStyleData>,
        ),
        (
            "DgnLsComponentData",
            trace_simple::<acadrust::objects::DgnLsComponentData>,
        ),
        (
            "DgnLsComponentType",
            trace_simple::<acadrust::objects::DgnLsComponentType>,
        ),
        (
            "DgnLsPhaseMode",
            trace_simple::<acadrust::objects::DgnLsPhaseMode>,
        ),
        ("DimSubtype", trace_simple::<acadrust::objects::DimSubtype>),
        ("Dimension", trace_simple::<acadrust::entities::Dimension>),
        (
            "DimensionType",
            trace_simple::<acadrust::entities::DimensionType>,
        ),
        (
            "DynamicBlockData",
            trace_simple::<acadrust::objects::DynamicBlockData>,
        ),
        (
            "EmbeddedEntity",
            trace_simple::<acadrust::entities::EmbeddedEntity>,
        ),
        (
            "ExtendedEntityData",
            trace_simple::<acadrust::entities::ExtendedEntityData>,
        ),
        (
            "FlowDirectionType",
            trace_simple::<acadrust::entities::FlowDirectionType>,
        ),
        (
            "HatchPatternType",
            trace_simple::<acadrust::entities::HatchPatternType>,
        ),
        (
            "HatchStyleType",
            trace_simple::<acadrust::entities::HatchStyleType>,
        ),
        (
            "HelixConstraint",
            trace_simple::<acadrust::entities::HelixConstraint>,
        ),
        (
            "HooklineDirection",
            trace_simple::<acadrust::entities::HooklineDirection>,
        ),
        (
            "HorizontalAlignment",
            trace_simple::<acadrust::entities::HorizontalAlignment>,
        ),
        (
            "LeaderContentType",
            trace_simple::<acadrust::entities::LeaderContentType>,
        ),
        (
            "LeaderCreationType",
            trace_simple::<acadrust::entities::LeaderCreationType>,
        ),
        (
            "LeaderDrawOrderType",
            trace_simple::<acadrust::objects::LeaderDrawOrderType>,
        ),
        (
            "LeaderPathType",
            trace_simple::<acadrust::entities::LeaderPathType>,
        ),
        (
            "LineTypeComplexContent",
            trace_simple::<acadrust::tables::LineTypeComplexContent>,
        ),
        (
            "LegacyEntityData",
            trace_simple::<acadrust::entities::LegacyEntityData>,
        ),
        (
            "MLineJustification",
            trace_simple::<acadrust::entities::MLineJustification>,
        ),
        ("MTextFlag", trace_simple::<acadrust::entities::MTextFlag>),
        (
            "MaterialProceduralValue",
            trace_simple::<acadrust::objects::MaterialProceduralValue>,
        ),
        (
            "MultiLeaderDrawOrderType",
            trace_simple::<acadrust::objects::MultiLeaderDrawOrderType>,
        ),
        (
            "MultiLeaderPathType",
            trace_simple::<acadrust::entities::MultiLeaderPathType>,
        ),
        (
            "ObjectContextKind",
            trace_simple::<acadrust::objects::ObjectContextKind>,
        ),
        (
            "NotificationType",
            trace_simple::<acadrust::notification::NotificationType>,
        ),
        (
            "OleFrameEnvelope",
            trace_simple::<acadrust::entities::OleFrameEnvelope>,
        ),
        (
            "OleObjectType",
            trace_simple::<acadrust::entities::OleObjectType>,
        ),
        (
            "PlotPaperUnits",
            trace_simple::<acadrust::objects::PlotPaperUnits>,
        ),
        (
            "PlotRotation",
            trace_simple::<acadrust::objects::PlotRotation>,
        ),
        ("PlotType", trace_simple::<acadrust::objects::PlotType>),
        (
            "PreviewFormat",
            trace_simple::<acadrust::document::PreviewFormat>,
        ),
        (
            "PolyfaceSmoothType",
            trace_simple::<acadrust::entities::PolyfaceSmoothType>,
        ),
        (
            "ProxyPayloadEncoding",
            trace_simple::<acadrust::objects::ProxyPayloadEncoding>,
        ),
        (
            "ResolutionUnit",
            trace_simple::<acadrust::objects::ResolutionUnit>,
        ),
        ("ScaledType", trace_simple::<acadrust::objects::ScaledType>),
        (
            "SemanticPropertyValue",
            trace_simple::<acadrust::objects::SemanticPropertyValue>,
        ),
        (
            "ShadePlotMode",
            trace_simple::<acadrust::objects::ShadePlotMode>,
        ),
        (
            "ShadePlotResolutionLevel",
            trace_simple::<acadrust::objects::ShadePlotResolutionLevel>,
        ),
        (
            "SolidHistoryOperation",
            trace_simple::<acadrust::objects::SolidHistoryOperation>,
        ),
        (
            "SurfaceData",
            trace_simple::<acadrust::entities::SurfaceData>,
        ),
        (
            "SurfaceKind",
            trace_simple::<acadrust::entities::SurfaceKind>,
        ),
        (
            "SurfaceSmoothType",
            trace_simple::<acadrust::entities::SurfaceSmoothType>,
        ),
        (
            "TableBorderType",
            trace_simple::<acadrust::objects::TableBorderType>,
        ),
        (
            "TableCellContentType",
            trace_simple::<acadrust::entities::TableCellContentType>,
        ),
        (
            "TableFlowDirection",
            trace_simple::<acadrust::objects::TableFlowDirection>,
        ),
        (
            "TextAlignmentType",
            trace_simple::<acadrust::entities::TextAlignmentType>,
        ),
        (
            "TextAngleType",
            trace_simple::<acadrust::entities::TextAngleType>,
        ),
        (
            "TextAttachmentDirectionType",
            trace_simple::<acadrust::entities::TextAttachmentDirectionType>,
        ),
        (
            "TextAttachmentPointType",
            trace_simple::<acadrust::entities::TextAttachmentPointType>,
        ),
        (
            "TextAttachmentType",
            trace_simple::<acadrust::entities::TextAttachmentType>,
        ),
        (
            "TextHorizontalAlignment",
            trace_simple::<acadrust::entities::TextHorizontalAlignment>,
        ),
        (
            "TextVerticalAlignment",
            trace_simple::<acadrust::entities::TextVerticalAlignment>,
        ),
        (
            "UnderlayType",
            trace_simple::<acadrust::entities::UnderlayType>,
        ),
        (
            "ValueUnitType",
            trace_simple::<acadrust::entities::ValueUnitType>,
        ),
        (
            "VbaDirectoryValue",
            trace_simple::<acadrust::vba::VbaDirectoryValue>,
        ),
        (
            "VerticalAlignment",
            trace_simple::<acadrust::entities::VerticalAlignment>,
        ),
        (
            "ViewportRenderMode",
            trace_simple::<acadrust::entities::ViewportRenderMode>,
        ),
        (
            "VisualStylePropertyValue",
            trace_simple::<acadrust::objects::VisualStylePropertyValue>,
        ),
        (
            "ViewRepSketchGeometry",
            trace_simple::<acadrust::objects::ViewRepSketchGeometry>,
        ),
        (
            "WipeoutClipMode",
            trace_simple::<acadrust::entities::WipeoutClipMode>,
        ),
        (
            "WipeoutClipType",
            trace_simple::<acadrust::entities::WipeoutClipType>,
        ),
        ("WireType", trace_simple::<acadrust::entities::WireType>),
    ];
    for (name, trace) in enums {
        trace(&mut tracer);
        eprintln!("[ocs_plugin_api build] traced enum variants: {name}");
    }

    let traced = tracer
        .registry()
        .expect("type registry tracing failed; see stderr for individual errors");
    let mut registry = map_to_custom_schema(&traced);
    let mut section_style_tracer = Tracer::new(TracerConfig::default());
    section_style_tracer
        .trace_simple_type::<acadrust::entities::SectionViewStyle>()
        .expect("section view style tracing failed");
    let section_style_registry = section_style_tracer
        .registry()
        .expect("section view style registry failed");
    let section_style = section_style_registry
        .get("SectionViewStyle")
        .expect("section view style registry entry");
    registry.types.insert(
        TypeId::new("EntitySectionViewStyle"),
        map_container("EntitySectionViewStyle", section_style),
    );
    let json = serde_json::to_string_pretty(&registry).unwrap();
    fs::write(out_dir.join("type_registry.json"), json).unwrap();
}

fn trace<T>(tracer: &mut Tracer, samples: &Samples)
where
    T: serde::Serialize + DeserializeOwned,
{
    let name = std::any::type_name::<T>();
    if let Err(e) = tracer.trace_type::<T>(samples) {
        eprintln!(
            "[ocs_plugin_api build] warning: tracing {} failed: {}",
            name, e
        );
    }
}

fn trace_simple<T>(tracer: &mut Tracer)
where
    T: DeserializeOwned,
{
    if let Err(error) = tracer.trace_simple_type::<T>() {
        panic!(
            "type registry enum tracing failed for {}: {error}",
            std::any::type_name::<T>()
        );
    }
}

fn add_enum_samples(tracer: &mut Tracer, samples: &mut Samples) {
    // serde-reflection needs at least one sample value per enum variant in
    // order to reconstruct the full schema. Provide samples for the enums that
    // need concrete serialized values.
    let _ = tracer.trace_value(samples, &acadrust::LineWeight::ByLayer);
    let _ = tracer.trace_value(samples, &acadrust::LineWeight::ByBlock);
    let _ = tracer.trace_value(samples, &acadrust::LineWeight::Default);
    let _ = tracer.trace_value(samples, &acadrust::LineWeight::Value(0));

    let _ = tracer.trace_value(samples, &acadrust::Transparency::BY_LAYER);
    let _ = tracer.trace_value(samples, &acadrust::Transparency::BY_BLOCK);
    let _ = tracer.trace_value(samples, &acadrust::Transparency::OPAQUE);

    let _ = tracer.trace_value(samples, &acadrust::entities::SmoothSurfaceType::None);
    let _ = tracer.trace_value(
        samples,
        &acadrust::entities::SmoothSurfaceType::QuadraticBSpline,
    );
    let _ = tracer.trace_value(
        samples,
        &acadrust::entities::SmoothSurfaceType::CubicBSpline,
    );
    let _ = tracer.trace_value(samples, &acadrust::entities::SmoothSurfaceType::Bezier);

    let _ = tracer.trace_value(samples, &acadrust::entities::AttachmentPoint::TopLeft);
    let _ = tracer.trace_value(samples, &acadrust::entities::AttachmentPoint::TopCenter);
    let _ = tracer.trace_value(samples, &acadrust::entities::AttachmentPoint::TopRight);
    let _ = tracer.trace_value(samples, &acadrust::entities::AttachmentPoint::MiddleLeft);
    let _ = tracer.trace_value(samples, &acadrust::entities::AttachmentPoint::MiddleCenter);
    let _ = tracer.trace_value(samples, &acadrust::entities::AttachmentPoint::MiddleRight);
    let _ = tracer.trace_value(samples, &acadrust::entities::AttachmentPoint::BottomLeft);
    let _ = tracer.trace_value(samples, &acadrust::entities::AttachmentPoint::BottomCenter);
    let _ = tracer.trace_value(samples, &acadrust::entities::AttachmentPoint::BottomRight);

    let _ = tracer.trace_value(samples, &acadrust::entities::DrawingDirection::LeftToRight);
    let _ = tracer.trace_value(samples, &acadrust::entities::DrawingDirection::TopToBottom);
    let _ = tracer.trace_value(samples, &acadrust::entities::DrawingDirection::ByStyle);

    let _ = tracer.trace_value(samples, &acadrust::entities::LineSpacingStyle::AtLeast);
    let _ = tracer.trace_value(samples, &acadrust::entities::LineSpacingStyle::Exactly);

    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::String(String::new()));
    let _ = tracer.trace_value(
        samples,
        &acadrust::xdata::XDataValue::ControlString(String::new()),
    );
    let _ = tracer.trace_value(
        samples,
        &acadrust::xdata::XDataValue::LayerName(String::new()),
    );
    let _ = tracer.trace_value(
        samples,
        &acadrust::xdata::XDataValue::BinaryData(Vec::new()),
    );
    let _ = tracer.trace_value(
        samples,
        &acadrust::xdata::XDataValue::Handle(acadrust::Handle::default()),
    );
    let _ = tracer.trace_value(
        samples,
        &acadrust::xdata::XDataValue::Point3D(acadrust::Vector3::default()),
    );
    let _ = tracer.trace_value(
        samples,
        &acadrust::xdata::XDataValue::Position3D(acadrust::Vector3::default()),
    );
    let _ = tracer.trace_value(
        samples,
        &acadrust::xdata::XDataValue::Displacement3D(acadrust::Vector3::default()),
    );
    let _ = tracer.trace_value(
        samples,
        &acadrust::xdata::XDataValue::Direction3D(acadrust::Vector3::default()),
    );
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Real(0.0));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Distance(0.0));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::ScaleFactor(0.0));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Integer16(0));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Integer32(0));

    // XRecord value variants
    let _ = tracer.trace_value(
        samples,
        &acadrust::objects::XRecordValue::String(String::new()),
    );
    let _ = tracer.trace_value(samples, &acadrust::objects::XRecordValue::Double(0.0));
    let _ = tracer.trace_value(samples, &acadrust::objects::XRecordValue::Int16(0));
    let _ = tracer.trace_value(samples, &acadrust::objects::XRecordValue::Int32(0));
    let _ = tracer.trace_value(samples, &acadrust::objects::XRecordValue::Int64(0));
    let _ = tracer.trace_value(samples, &acadrust::objects::XRecordValue::Byte(0));
    let _ = tracer.trace_value(samples, &acadrust::objects::XRecordValue::Bool(false));
    let _ = tracer.trace_value(
        samples,
        &acadrust::objects::XRecordValue::Handle(acadrust::Handle::default()),
    );
    let _ = tracer.trace_value(
        samples,
        &acadrust::objects::XRecordValue::Point3D(0.0, 0.0, 0.0),
    );
    let _ = tracer.trace_value(samples, &acadrust::objects::XRecordValue::Chunk(Vec::new()));

    // Proxy reference kinds pulled in by XRecord.object_references
    let _ = tracer.trace_value(samples, &acadrust::objects::ProxyReferenceKind::Undefined);
    let _ = tracer.trace_value(
        samples,
        &acadrust::objects::ProxyReferenceKind::SoftOwnership,
    );
    let _ = tracer.trace_value(
        samples,
        &acadrust::objects::ProxyReferenceKind::HardOwnership,
    );
    let _ = tracer.trace_value(samples, &acadrust::objects::ProxyReferenceKind::SoftPointer);
    let _ = tracer.trace_value(samples, &acadrust::objects::ProxyReferenceKind::HardPointer);

    // KnownXRecordKind variants
    let _ = tracer.trace_value(
        samples,
        &acadrust::objects::KnownXRecordKind::LayerViewportAlphaOverride,
    );
    let _ = tracer.trace_value(samples, &acadrust::objects::KnownXRecordKind::Unknown);
}

fn map_to_custom_schema(traced: &Registry) -> TypeRegistry {
    let mut types = BTreeMap::new();
    for (name, format) in traced.iter() {
        let info = map_container(name, format);
        types.insert(TypeId(name.clone()), info);
    }
    TypeRegistry { types }
}

fn map_container(name: &str, format: &ContainerFormat) -> TypeInfo {
    match format {
        ContainerFormat::Struct(fields) => TypeInfo {
            name: TypeId(name.to_string()),
            kind: TypeKind::Struct,
            fields: fields.iter().map(map_field).collect(),
            variants: vec![],
            methods: vec![],
            doc: None,
        },
        ContainerFormat::Enum(variants) => TypeInfo {
            name: TypeId(name.to_string()),
            kind: TypeKind::Enum,
            fields: vec![],
            variants: variants
                .iter()
                .map(|(idx, v)| map_variant(*idx, v))
                .collect(),
            methods: vec![],
            doc: None,
        },
        ContainerFormat::NewTypeStruct(format) => TypeInfo {
            name: TypeId(name.to_string()),
            kind: TypeKind::Newtype,
            fields: vec![FieldInfo {
                name: "0".to_string(),
                ..map_format_field(format)
            }],
            variants: vec![],
            methods: vec![],
            doc: None,
        },
        ContainerFormat::TupleStruct(formats) => TypeInfo {
            name: TypeId(name.to_string()),
            kind: TypeKind::Tuple,
            fields: formats
                .iter()
                .enumerate()
                .map(|(i, f)| FieldInfo {
                    name: i.to_string(),
                    ..map_format_field(f)
                })
                .collect(),
            variants: vec![],
            methods: vec![],
            doc: None,
        },
        ContainerFormat::UnitStruct => TypeInfo {
            name: TypeId(name.to_string()),
            kind: TypeKind::Unit,
            fields: vec![],
            variants: vec![],
            methods: vec![],
            doc: None,
        },
    }
}

fn map_field(named: &Named<Format>) -> FieldInfo {
    let mut field = map_format_field(&named.value);
    field.name = named.name.clone();
    field
}

fn map_format_field(format: &Format) -> FieldInfo {
    if let Some(name) = primitive_format_name(format) {
        return FieldInfo {
            name: String::new(),
            type_id: TypeId(name),
            optional: false,
            is_sequence: false,
        };
    }
    match format {
        Format::TypeName(name) => FieldInfo {
            name: String::new(),
            type_id: TypeId(name.clone()),
            optional: false,
            is_sequence: false,
        },
        Format::Option(inner) => {
            let mut field = map_format_field(inner);
            field.optional = true;
            field
        }
        Format::Seq(inner) => {
            let mut field = map_format_field(inner);
            field.is_sequence = true;
            field
        }
        Format::TupleArray { content, size } => {
            let mut field = map_format_field(content);
            field.type_id = TypeId(format!("[{}; {}]", type_id_of_format(content), size));
            field.is_sequence = true;
            field
        }
        Format::Map { key, value } => FieldInfo {
            name: String::new(),
            type_id: TypeId(format!(
                "Map<{}, {}>",
                type_id_of_format(key),
                type_id_of_format(value)
            )),
            optional: false,
            is_sequence: false,
        },
        Format::Tuple(formats) => FieldInfo {
            name: String::new(),
            type_id: TypeId(format!(
                "({})",
                formats
                    .iter()
                    .map(type_id_of_format)
                    .collect::<Vec<_>>()
                    .join(",")
            )),
            optional: false,
            is_sequence: false,
        },
        Format::Variable(_) => FieldInfo {
            name: String::new(),
            type_id: TypeId("Value".to_string()),
            optional: false,
            is_sequence: false,
        },
        _ => FieldInfo {
            name: String::new(),
            type_id: TypeId("unknown".to_string()),
            optional: false,
            is_sequence: false,
        },
    }
}

fn primitive_format_name(format: &Format) -> Option<String> {
    let name = match format {
        Format::Unit => "()",
        Format::Bool => "bool",
        Format::I8 => "i8",
        Format::I16 => "i16",
        Format::I32 => "i32",
        Format::I64 => "i64",
        Format::I128 => "i128",
        Format::U8 => "u8",
        Format::U16 => "u16",
        Format::U32 => "u32",
        Format::U64 => "u64",
        Format::U128 => "u128",
        Format::F32 => "f32",
        Format::F64 => "f64",
        Format::Char => "char",
        Format::Str => "String",
        Format::Bytes => "bytes",
        _ => return None,
    };
    Some(name.to_string())
}

fn type_id_of_format(format: &Format) -> String {
    if let Some(name) = primitive_format_name(format) {
        return name;
    }
    match format {
        Format::TypeName(name) => name.clone(),
        Format::Option(inner) => format!("Option<{}>", type_id_of_format(inner)),
        Format::Seq(inner) => format!("Vec<{}>", type_id_of_format(inner)),
        Format::TupleArray { content, size } => {
            format!("[{}; {}]", type_id_of_format(content), size)
        }
        Format::Map { key, value } => format!(
            "Map<{}, {}>",
            type_id_of_format(key),
            type_id_of_format(value)
        ),
        Format::Tuple(formats) => format!(
            "({})",
            formats
                .iter()
                .map(type_id_of_format)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Format::Variable(_) => "Value".to_string(),
        _ => "unknown".to_string(),
    }
}

fn map_variant(discriminant: u32, named: &Named<VariantFormat>) -> EnumVariantInfo {
    let fields = match &named.value {
        VariantFormat::Unit => vec![],
        VariantFormat::Variable(_) => vec![],
        VariantFormat::NewType(format) => vec![FieldInfo {
            name: "0".to_string(),
            ..map_format_field(format)
        }],
        VariantFormat::Tuple(formats) => formats
            .iter()
            .enumerate()
            .map(|(i, f)| FieldInfo {
                name: i.to_string(),
                ..map_format_field(f)
            })
            .collect(),
        VariantFormat::Struct(fields) => fields.iter().map(map_field).collect(),
    };
    EnumVariantInfo {
        name: named.name.clone(),
        discriminant,
        fields,
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Version info
// ════════════════════════════════════════════════════════════════════════════

fn generate_version_info(out_dir: &Path) {
    let lock_path = workspace_cargo_lock_path();
    let lockfile = Lockfile::load(&lock_path).expect("load Cargo.lock");

    // Use Cargo.lock's mtime as the build timestamp so the embedded JSON stays
    // stable across normal incremental builds and only changes when dependencies
    // are updated. Stored as Unix seconds to avoid an extra date-formatting
    // dependency in the build script.
    let build_timestamp = fs::metadata(&lock_path)
        .and_then(|m| m.modified())
        .and_then(|t| {
            t.duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .map_err(std::io::Error::other)
        })
        .unwrap_or(0);

    let ocs = lockfile
        .packages
        .iter()
        .find(|p| p.name.as_str() == "OpenCADStudio")
        .expect("OpenCADStudio package in Cargo.lock");
    let acadrust = lockfile
        .packages
        .iter()
        .find(|p| p.name.as_str() == "acadrust")
        .expect("acadrust package in Cargo.lock");

    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let rustc_version = Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let info = serde_json::json!({
        "ocs_version": ocs.version.to_string(),
        "ocs_plugin_api_version": env!("CARGO_PKG_VERSION"),
        "acadrust_version": acadrust.version.to_string(),
        "acadrust_source": acadrust.source.as_ref().map(|s| s.to_string()),
        "rustc_version": rustc_version,
        "api_version": 6,
        "api_version_min_supported": 2,
        "build_timestamp": build_timestamp,
    });
    fs::write(out_dir.join("version_info.json"), info.to_string()).unwrap();
}

fn workspace_cargo_lock_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    // CARGO_MANIFEST_DIR is crates/ocs_plugin_api; walk up to the workspace root.
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("Cargo.lock"))
        .expect("Cargo.lock in workspace root")
}
