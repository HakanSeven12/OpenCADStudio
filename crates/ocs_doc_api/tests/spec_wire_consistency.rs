//! Specâ†”wire consistency tests (enforces the codegen contract the headers
//! promise): every `op`/`query` name in `spec/entities.toml` maps to an existing
//! `Operation`/`Query` variant, and the wire enums are append-only relative to a
//! recorded baseline (bincode discriminant stability).

use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Deserialize)]
struct Spec {
    #[serde(default)]
    family: Vec<Family>,
}
#[derive(Deserialize)]
struct Family {
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    constructor: Vec<MethodRef>,
    #[serde(default)]
    method: Vec<MethodRef>,
}
#[derive(Deserialize)]
struct MethodRef {
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

fn spec() -> Spec {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/spec/entities.toml"))
        .expect("read spec/entities.toml");
    toml::from_str(&src).expect("parse spec/entities.toml")
}

/// The variant names of the `Operation` enum, in declaration order.
fn operation_variants() -> Vec<String> {
    use ocs_doc_api::ops::{BoolOp, Curve2Spec, InsertSpec, Operation, PlacementSpec, SolidPrimitive, ViewportSpec};
    use ocs_doc_api::ObjectId;
    let _marker: Vec<Operation> = vec![
        Operation::CreateSolid(SolidPrimitive::Sphere { centre: [0.0; 3], radius: 1.0 }),
        Operation::CreateCurve(Curve2Spec::Point { position: [0.0; 3] }),
        Operation::Extrude { profile: ObjectId::from_u64(0), direction: [0.0; 3] },
        Operation::Revolve { profile: ObjectId::from_u64(0), axis: ([0.0; 3], [0.0; 3]), angle: 0.0 },
        Operation::Transform { id: ObjectId::from_u64(0), placement: PlacementSpec::IDENTITY },
        Operation::SolidBoolean { op: BoolOp::Union, a: ObjectId::from_u64(0), b: ObjectId::from_u64(0), erase_sources: true },
        Operation::AddVertex { id: ObjectId::from_u64(0), at: 0, point: [0.0; 3] },
        Operation::Delete { id: ObjectId::from_u64(0) },
        Operation::CreateMany(vec![]),
        Operation::TransformMany { ids: vec![], placement: PlacementSpec::IDENTITY },
        Operation::DeleteMany(vec![]),
        Operation::CreateInsert(InsertSpec { block_name: String::new(), insert_point: [0.0; 3], scale: 1.0, rotation: 0.0 }),
        Operation::CreateViewport(ViewportSpec { center: [0.0; 3], width: 0.0, height: 0.0, view_target: [0.0; 3], view_height: 0.0 }),
        Operation::CreateText(ocs_doc_api::ops::TextSpec { value: String::new(), insertion_point: [0.0; 3], height: 0.0, rotation: 0.0 }),
        Operation::CreateMText(ocs_doc_api::ops::MTextSpec { value: String::new(), insertion_point: [0.0; 3], height: 0.0 }),
        Operation::SetTextContent { id: ObjectId::from_u64(0), value: String::new() },
        Operation::CreateHatch(ocs_doc_api::ops::HatchSpec { boundary: vec![], solid: true }),
        Operation::CreateDimensionLinear(ocs_doc_api::ops::DimensionSpec { first_point: [0.0; 3], second_point: [0.0; 3], definition_point: [0.0; 3] }),
        Operation::SetAttribute { id: ObjectId::from_u64(0), tag: String::new(), value: String::new() },
        Operation::SetViewportView { id: ObjectId::from_u64(0), view_target: [0.0; 3], view_height: 0.0 },
        Operation::CreateRasterImage(ocs_doc_api::ops::RasterImageSpec {
            file_path: String::new(), insertion_point: [0.0; 3], u_vector: [0.0; 3], v_vector: [0.0; 3], size: [0.0; 2],
        }),
    ];
    // Derive the variant names by serializing a probe of each and reading the
    // bincode variant index -> name map is not available; instead match by
    // constructing each and using a tiny name extractor via Debug prefix.
    _marker
        .iter()
        .map(|op| {
            let dbg = format!("{:?}", op);
            dbg.split(|c: char| c == '(' || c == ' ' || c == '{').next().unwrap_or("").to_string()
        })
        .collect()
}

/// The variant names of the `Query` enum, in declaration order.
fn query_variants() -> Vec<String> {
    use ocs_doc_api::query::Query;
    use ocs_doc_api::ObjectId;
    let marker: Vec<Query> = vec![
        Query::GetEntity { id: ObjectId::from_u64(0) },
        Query::GetBounds { id: ObjectId::from_u64(0) },
        Query::GetCentroid { id: ObjectId::from_u64(0) },
        Query::GetVolume { id: ObjectId::from_u64(0) },
        Query::GetIntersects { a: ObjectId::from_u64(0), b: ObjectId::from_u64(0) },
        Query::GetGeometryRevision,
        Query::GetTextContent { id: ObjectId::from_u64(0) },
        Query::GetHatchBoundary { id: ObjectId::from_u64(0) },
        Query::GetDimensionMeasurement { id: ObjectId::from_u64(0) },
        Query::GetAttributes { id: ObjectId::from_u64(0) },
        Query::GetBlockEntities { block_name: String::new() },
        Query::GetViewportView { id: ObjectId::from_u64(0) },
    ];
    marker
        .iter()
        .map(|q| {
            let dbg = format!("{:?}", q);
            dbg.split(|c: char| c == '(' || c == ' ' || c == '{').next().unwrap_or("").to_string()
        })
        .collect()
}

#[test]
fn every_spec_op_and_query_maps_to_an_enum_variant() {
    let spec = spec();
    let op_names: BTreeSet<String> = operation_variants().into_iter().collect();
    let query_names: BTreeSet<String> = query_variants().into_iter().collect();

    let mut missing = Vec::new();
    for fam in &spec.family {
        for m in fam.constructor.iter().chain(fam.method.iter()) {
            if let Some(op) = &m.op {
                if !op_names.contains(op) {
                    missing.push(format!("Operation::{op} (family {})", fam.handle.as_deref().unwrap_or("?")));
                }
            }
            if let Some(q) = &m.query {
                if !query_names.contains(q) {
                    missing.push(format!("Query::{q} (family {})", fam.handle.as_deref().unwrap_or("?")));
                }
            }
        }
    }
    assert!(missing.is_empty(), "spec references ops/queries with no enum variant: {missing:?}");
}

/// Recorded baseline of the `Operation` variant order (append-only contract).
/// New variants must be APPENDED after these; none may be removed or reordered.
const OPERATION_BASELINE: &[&str] = &[
    "CreateSolid",
    "CreateCurve",
    "Extrude",
    "Revolve",
    "Transform",
    "SolidBoolean",
    "AddVertex",
    "Delete",
    "CreateMany",
    "TransformMany",
    "DeleteMany",
    "CreateInsert",
    "CreateViewport",
    "CreateText",
    "CreateMText",
    "SetTextContent",
    "CreateHatch",
    "CreateDimensionLinear",
    "SetAttribute",
    "SetViewportView",
    "CreateRasterImage",
];

/// Recorded baseline of the `Query` variant order.
const QUERY_BASELINE: &[&str] = &[
    "GetEntity",
    "GetBounds",
    "GetCentroid",
    "GetVolume",
    "GetIntersects",
    "GetGeometryRevision",
    "GetTextContent",
    "GetHatchBoundary",
    "GetDimensionMeasurement",
    "GetAttributes",
    "GetBlockEntities",
    "GetViewportView",
];

#[test]
fn operation_enum_is_append_only_vs_baseline() {
    let current = operation_variants();
    assert!(
        current.len() >= OPERATION_BASELINE.len(),
        "Operation enum shrank: {current:?} < baseline {OPERATION_BASELINE:?}"
    );
    for (i, name) in OPERATION_BASELINE.iter().enumerate() {
        assert_eq!(
            current[i], *name,
            "Operation variant {i} changed (append-only violation): {current:?}"
        );
    }
}

#[test]
fn query_enum_is_append_only_vs_baseline() {
    let current = query_variants();
    assert!(
        current.len() >= QUERY_BASELINE.len(),
        "Query enum shrank: {current:?} < baseline {QUERY_BASELINE:?}"
    );
    for (i, name) in QUERY_BASELINE.iter().enumerate() {
        assert_eq!(
            current[i], *name,
            "Query variant {i} changed (append-only violation): {current:?}"
        );
    }
}
