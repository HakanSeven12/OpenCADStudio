// FBX export — the drawing's 3D meshes as a binary FBX 7.4 file for
// visualisation tools. Each mesh becomes one model with its own geometry and a
// material in its display colour; coordinates keep the drawing's Z-up axes and
// the unit scale says how many centimetres one drawing unit is.
//
// Binary layout (version 7400): a 27-byte header, then node records. A record
// is its end offset, property count and property byte length (u32 each), its
// name (u8 length + bytes), the properties, its child records, and a 13-byte
// null record closing the child list. The top level ends with a null record
// and the fixed footer readers check.

use crate::scene::model::mesh_model::MeshModel;

const VERSION: u32 = 7400;
const HEADER: &[u8] = b"Kaydara FBX Binary  \x00\x1a\x00";
const FILE_ID: [u8; 16] = [
    0x28, 0xb3, 0x2a, 0xeb, 0xb6, 0x24, 0xcc, 0xc2, 0xbf, 0xc8, 0xb0, 0x2a, 0xa9, 0x2b, 0xfc, 0xf1,
];
const CREATION_TIME: &str = "1970-01-01 10:00:00:000";
const FOOTER_ID: [u8; 16] = [
    0xfa, 0xbc, 0xab, 0x09, 0xd0, 0xc8, 0xd4, 0x66, 0xb1, 0x76, 0xfb, 0x83, 0x1c, 0xf7, 0x26, 0x7e,
];
const FOOTER_MAGIC: [u8; 16] = [
    0xf8, 0x5a, 0x8c, 0x6a, 0xde, 0xf5, 0xd9, 0x7e, 0xec, 0xe9, 0x0c, 0xe3, 0x75, 0x8f, 0x29, 0x0b,
];

enum Prop {
    Bool(bool),
    I32(i32),
    I64(i64),
    F64(f64),
    Str(String),
    Raw(Vec<u8>),
    I32s(Vec<i32>),
    F64s(Vec<f64>),
}

struct Node {
    name: &'static str,
    props: Vec<Prop>,
    children: Vec<Node>,
}

fn node(name: &'static str, props: Vec<Prop>, children: Vec<Node>) -> Node {
    Node {
        name,
        props,
        children,
    }
}

fn s(value: &str) -> Prop {
    Prop::Str(value.to_string())
}

/// `P: name, type, label, flags, values…` inside a Properties70 block.
fn p(name: &str, kind: &str, label: &str, values: Vec<Prop>) -> Node {
    let mut props = vec![s(name), s(kind), s(label), s("")];
    props.extend(values);
    node("P", props, Vec::new())
}

fn write_prop(out: &mut Vec<u8>, prop: &Prop) {
    let array = |out: &mut Vec<u8>, code: u8, count: usize, bytes: Vec<u8>| {
        out.push(code);
        out.extend((count as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend((bytes.len() as u32).to_le_bytes());
        out.extend(bytes);
    };
    match prop {
        Prop::Bool(value) => out.extend([b'C', u8::from(*value)]),
        Prop::I32(value) => {
            out.push(b'I');
            out.extend(value.to_le_bytes());
        }
        Prop::I64(value) => {
            out.push(b'L');
            out.extend(value.to_le_bytes());
        }
        Prop::F64(value) => {
            out.push(b'D');
            out.extend(value.to_le_bytes());
        }
        Prop::Str(value) => {
            out.push(b'S');
            out.extend((value.len() as u32).to_le_bytes());
            out.extend(value.as_bytes());
        }
        Prop::Raw(value) => {
            out.push(b'R');
            out.extend((value.len() as u32).to_le_bytes());
            out.extend(value);
        }
        Prop::I32s(values) => array(
            out,
            b'i',
            values.len(),
            values.iter().flat_map(|v| v.to_le_bytes()).collect(),
        ),
        Prop::F64s(values) => array(
            out,
            b'd',
            values.len(),
            values.iter().flat_map(|v| v.to_le_bytes()).collect(),
        ),
    }
}

fn write_node(out: &mut Vec<u8>, node: &Node) {
    let start = out.len();
    out.extend([0u8; 12]);
    out.push(node.name.len() as u8);
    out.extend(node.name.as_bytes());
    let props_start = out.len();
    for prop in &node.props {
        write_prop(out, prop);
    }
    let props_len = (out.len() - props_start) as u32;
    if !node.children.is_empty() || node.props.is_empty() {
        for child in &node.children {
            write_node(out, child);
        }
        out.extend([0u8; 13]);
    }
    let end = out.len() as u32;
    out[start..start + 4].copy_from_slice(&end.to_le_bytes());
    out[start + 4..start + 8].copy_from_slice(&(node.props.len() as u32).to_le_bytes());
    out[start + 8..start + 12].copy_from_slice(&props_len.to_le_bytes());
}

/// Geometry, model and material of one mesh, with the connections that place
/// them in the scene. `None` for a mesh without triangles.
fn mesh_nodes(mesh: &MeshModel, index: usize) -> Option<([Node; 3], [Node; 3])> {
    let points: Vec<[f64; 3]> = mesh
        .verts
        .iter()
        .enumerate()
        .map(|(k, high)| {
            let low = mesh.verts_low.get(k).copied().unwrap_or([0.0; 3]);
            std::array::from_fn(|axis| high[axis] as f64 + low[axis] as f64)
        })
        .collect();
    let mut polygon_indices = Vec::with_capacity(mesh.indices.len());
    let mut normals = Vec::with_capacity(mesh.indices.len() * 3);
    for triangle in mesh.indices.chunks_exact(3) {
        if triangle.iter().any(|&k| k as usize >= points.len()) {
            continue;
        }
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]].map(|k| points[k as usize]);
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length <= 0.0 {
            continue;
        }
        // The last corner of a polygon is stored as its bitwise complement.
        polygon_indices.extend([
            triangle[0] as i32,
            triangle[1] as i32,
            !(triangle[2] as i32),
        ]);
        for _ in 0..3 {
            normals.extend(n.map(|value| value / length));
        }
    }
    if polygon_indices.is_empty() {
        return None;
    }
    let base = 1_000_000 + index as i64 * 3;
    let (geometry_id, model_id, material_id) = (base, base + 1, base + 2);
    let name = format!("Mesh{}", index + 1);
    let geometry = node(
        "Geometry",
        vec![
            Prop::I64(geometry_id),
            s(&format!("{name}\x00\x01Geometry")),
            s("Mesh"),
        ],
        vec![
            node("Vertices", vec![Prop::F64s(points.concat())], Vec::new()),
            node(
                "PolygonVertexIndex",
                vec![Prop::I32s(polygon_indices)],
                Vec::new(),
            ),
            node("GeometryVersion", vec![Prop::I32(124)], Vec::new()),
            node(
                "LayerElementNormal",
                vec![Prop::I32(0)],
                vec![
                    node("Version", vec![Prop::I32(101)], Vec::new()),
                    node("Name", vec![s("")], Vec::new()),
                    node(
                        "MappingInformationType",
                        vec![s("ByPolygonVertex")],
                        Vec::new(),
                    ),
                    node("ReferenceInformationType", vec![s("Direct")], Vec::new()),
                    node("Normals", vec![Prop::F64s(normals)], Vec::new()),
                ],
            ),
            node(
                "LayerElementMaterial",
                vec![Prop::I32(0)],
                vec![
                    node("Version", vec![Prop::I32(101)], Vec::new()),
                    node("Name", vec![s("")], Vec::new()),
                    node("MappingInformationType", vec![s("AllSame")], Vec::new()),
                    node(
                        "ReferenceInformationType",
                        vec![s("IndexToDirect")],
                        Vec::new(),
                    ),
                    node("Materials", vec![Prop::I32s(vec![0])], Vec::new()),
                ],
            ),
            node(
                "Layer",
                vec![Prop::I32(0)],
                vec![
                    node("Version", vec![Prop::I32(100)], Vec::new()),
                    node(
                        "LayerElement",
                        Vec::new(),
                        vec![
                            node("Type", vec![s("LayerElementNormal")], Vec::new()),
                            node("TypedIndex", vec![Prop::I32(0)], Vec::new()),
                        ],
                    ),
                    node(
                        "LayerElement",
                        Vec::new(),
                        vec![
                            node("Type", vec![s("LayerElementMaterial")], Vec::new()),
                            node("TypedIndex", vec![Prop::I32(0)], Vec::new()),
                        ],
                    ),
                ],
            ),
        ],
    );
    let model = node(
        "Model",
        vec![
            Prop::I64(model_id),
            s(&format!("{name}\x00\x01Model")),
            s("Mesh"),
        ],
        vec![
            node("Version", vec![Prop::I32(232)], Vec::new()),
            node("Properties70", Vec::new(), Vec::new()),
            node("Shading", vec![Prop::Bool(true)], Vec::new()),
            node("Culling", vec![s("CullingOff")], Vec::new()),
        ],
    );
    let [r, g, b, _] = mesh.color.map(f64::from);
    let material = node(
        "Material",
        vec![
            Prop::I64(material_id),
            s(&format!("{name}\x00\x01Material")),
            s(""),
        ],
        vec![
            node("Version", vec![Prop::I32(102)], Vec::new()),
            node("ShadingModel", vec![s("phong")], Vec::new()),
            node("MultiLayer", vec![Prop::I32(0)], Vec::new()),
            node(
                "Properties70",
                Vec::new(),
                vec![p(
                    "DiffuseColor",
                    "Color",
                    "",
                    vec![Prop::F64(r), Prop::F64(g), Prop::F64(b)],
                )],
            ),
        ],
    );
    let link = |child: i64, parent: i64| {
        node(
            "C",
            vec![s("OO"), Prop::I64(child), Prop::I64(parent)],
            Vec::new(),
        )
    };
    Some((
        [geometry, model, material],
        [
            link(model_id, 0),
            link(geometry_id, model_id),
            link(material_id, model_id),
        ],
    ))
}

/// A binary FBX file holding `meshes`; `unit_cm` is the length of one drawing
/// unit in centimetres. `None` when no mesh has a triangle.
pub fn build_fbx(meshes: &[&MeshModel], unit_cm: f64) -> Option<Vec<u8>> {
    let mut objects = Vec::new();
    let mut connections = Vec::new();
    for (index, mesh) in meshes.iter().enumerate() {
        if let Some((nodes, links)) = mesh_nodes(mesh, index) {
            objects.extend(nodes);
            connections.extend(links);
        }
    }
    if objects.is_empty() {
        return None;
    }
    let count = |kind: &str| objects.iter().filter(|o| o.name == kind).count() as i32;
    let object_type = |kind: &'static str, n: i32| {
        node(
            "ObjectType",
            vec![s(kind)],
            vec![node("Count", vec![Prop::I32(n)], Vec::new())],
        )
    };
    let (models, geometries, materials) = (count("Model"), count("Geometry"), count("Material"));
    let axis = |name: &str, value: i32| p(name, "int", "Integer", vec![Prop::I32(value)]);
    let root = [
        node(
            "FBXHeaderExtension",
            Vec::new(),
            vec![
                node("FBXHeaderVersion", vec![Prop::I32(1003)], Vec::new()),
                node("FBXVersion", vec![Prop::I32(VERSION as i32)], Vec::new()),
                node("Creator", vec![s("OpenCADStudio")], Vec::new()),
            ],
        ),
        node("FileId", vec![Prop::Raw(FILE_ID.to_vec())], Vec::new()),
        node("CreationTime", vec![s(CREATION_TIME)], Vec::new()),
        node("Creator", vec![s("OpenCADStudio")], Vec::new()),
        node(
            "GlobalSettings",
            Vec::new(),
            vec![
                node("Version", vec![Prop::I32(1000)], Vec::new()),
                node(
                    "Properties70",
                    Vec::new(),
                    vec![
                        axis("UpAxis", 2),
                        axis("UpAxisSign", 1),
                        axis("FrontAxis", 1),
                        axis("FrontAxisSign", -1),
                        axis("CoordAxis", 0),
                        axis("CoordAxisSign", 1),
                        p(
                            "UnitScaleFactor",
                            "double",
                            "Number",
                            vec![Prop::F64(unit_cm)],
                        ),
                    ],
                ),
            ],
        ),
        node(
            "Definitions",
            Vec::new(),
            vec![
                node("Version", vec![Prop::I32(100)], Vec::new()),
                node(
                    "Count",
                    vec![Prop::I32(1 + models + geometries + materials)],
                    Vec::new(),
                ),
                object_type("GlobalSettings", 1),
                object_type("Model", models),
                object_type("Geometry", geometries),
                object_type("Material", materials),
            ],
        ),
        node("Objects", Vec::new(), objects),
        node("Connections", Vec::new(), connections),
    ];

    let mut out = Vec::new();
    out.extend(HEADER);
    out.extend(VERSION.to_le_bytes());
    for node in &root {
        write_node(&mut out, node);
    }
    out.extend([0u8; 13]);
    out.extend(FOOTER_ID);
    out.extend([0u8; 4]);
    let pad = match (16 - out.len() % 16) % 16 {
        0 => 16,
        pad => pad,
    };
    out.extend(std::iter::repeat_n(0u8, pad));
    out.extend(VERSION.to_le_bytes());
    out.extend([0u8; 120]);
    out.extend(FOOTER_MAGIC);
    Some(out)
}

/// Web: no save dialog — the export continues with a virtual filename and
/// the generated bytes are delivered as a browser download.
#[cfg(target_arch = "wasm32")]
pub async fn pick_fbx_path_owned(stem: String) -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(format!("{stem}.fbx")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk the records the way a reader does and return the top-level
    /// names, checking every end offset lands inside its parent.
    fn top_level_names(bytes: &[u8]) -> Vec<String> {
        fn u32_at(bytes: &[u8], at: usize) -> usize {
            u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
        }
        fn walk(
            bytes: &[u8],
            mut at: usize,
            end: usize,
            mut names: Option<&mut Vec<String>>,
        ) -> usize {
            loop {
                let record_end = u32_at(bytes, at);
                if record_end == 0 {
                    return at + 13;
                }
                assert!(record_end <= end, "record overruns its parent");
                let name_len = bytes[at + 12] as usize;
                if let Some(names) = names.as_deref_mut() {
                    let name = &bytes[at + 13..at + 13 + name_len];
                    names.push(String::from_utf8(name.to_vec()).unwrap());
                }
                let children = at + 13 + name_len + u32_at(bytes, at + 8);
                if children < record_end {
                    assert_eq!(walk(bytes, children, record_end, None), record_end);
                }
                at = record_end;
            }
        }
        let mut names = Vec::new();
        let end = walk(bytes, 27, bytes.len(), Some(&mut names));
        assert_eq!(&bytes[end..end + 16], &FOOTER_ID);
        assert_eq!(&bytes[bytes.len() - 16..], &FOOTER_MAGIC);
        names
    }

    fn mesh(verts: Vec<[f32; 3]>, indices: Vec<u32>) -> MeshModel {
        MeshModel {
            name: String::new(),
            verts,
            verts_low: Vec::new(),
            normals: Vec::new(),
            indices,
            triangle_material_handles: Vec::new(),
            triangle_colors: Vec::new(),
            color: [1.0; 4],
            selected: false,
        }
    }

    #[test]
    fn a_triangle_writes_a_well_formed_file() {
        let triangle = mesh(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let bytes = build_fbx(&[&triangle], 0.1).expect("fbx");
        assert!(bytes.starts_with(HEADER));
        assert_eq!(
            top_level_names(&bytes),
            [
                "FBXHeaderExtension",
                "FileId",
                "CreationTime",
                "Creator",
                "GlobalSettings",
                "Definitions",
                "Objects",
                "Connections",
            ]
        );
        let empty = mesh(Vec::new(), Vec::new());
        assert!(build_fbx(&[&empty], 1.0).is_none());
    }
}
