use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
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

    // Curated allow-list of acadrust types. We intentionally exclude
    // EntityType here to avoid pulling in all 41+ entity variants and any
    // custom-serializer edge cases.
    type TraceFn = fn(&mut Tracer, &Samples);
    let types: Vec<(&str, TraceFn)> = vec![
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
    ];

    for (name, f) in types {
        f(&mut tracer, &samples);
        // serde-reflection accumulates named types as it traces; we only need
        // to ensure the seed types are recorded even if a nested trace fails.
        eprintln!("[ocs_plugin_api build] traced type registry entry: {}", name);
    }

    let traced = tracer
        .registry()
        .expect("type registry tracing failed; see stderr for individual errors");
    let registry = map_to_custom_schema(&traced);
    let json = serde_json::to_string_pretty(&registry).unwrap();
    fs::write(out_dir.join("type_registry.json"), json).unwrap();
}

fn trace<T>(tracer: &mut Tracer, samples: &Samples)
where
    T: serde::Serialize + DeserializeOwned,
{
    let name = std::any::type_name::<T>();
    if let Err(e) = tracer.trace_type::<T>(samples) {
        eprintln!("[ocs_plugin_api build] warning: tracing {} failed: {}", name, e);
    }
}

fn add_enum_samples(tracer: &mut Tracer, samples: &mut Samples) {
    // serde-reflection needs at least one sample value per enum variant in
    // order to reconstruct the full schema. Provide samples for the enums that
    // appear inside the allow-list types below.
    let _ = tracer.trace_value(samples, &acadrust::LineWeight::ByLayer);
    let _ = tracer.trace_value(samples, &acadrust::LineWeight::ByBlock);
    let _ = tracer.trace_value(samples, &acadrust::LineWeight::Default);
    let _ = tracer.trace_value(samples, &acadrust::LineWeight::Value(0));

    let _ = tracer.trace_value(samples, &acadrust::entities::SmoothSurfaceType::None);
    let _ = tracer.trace_value(samples, &acadrust::entities::SmoothSurfaceType::QuadraticBSpline);
    let _ = tracer.trace_value(samples, &acadrust::entities::SmoothSurfaceType::CubicBSpline);
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
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::ControlString(String::new()));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::LayerName(String::new()));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::BinaryData(Vec::new()));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Handle(acadrust::Handle::default()));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Point3D(acadrust::Vector3::default()));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Position3D(acadrust::Vector3::default()));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Displacement3D(acadrust::Vector3::default()));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Direction3D(acadrust::Vector3::default()));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Real(0.0));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Distance(0.0));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::ScaleFactor(0.0));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Integer16(0));
    let _ = tracer.trace_value(samples, &acadrust::xdata::XDataValue::Integer32(0));
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

    let info = serde_json::json!({
        "ocs_version": ocs.version.to_string(),
        "ocs_plugin_api_version": env!("CARGO_PKG_VERSION"),
        "acadrust_version": acadrust.version.to_string(),
        "acadrust_source": acadrust.source.as_ref().map(|s| s.to_string()),
        "api_version": 4,
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
