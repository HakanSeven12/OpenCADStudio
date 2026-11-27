// glTF 2.0 mesh importer (.gltf text with its buffers, or binary .glb).
//
// Each mesh a scene node places becomes one MESH entity of its triangles,
// with the node's transform applied and coincident corners joined, so the
// drawing keeps, snaps, saves and plots it like a MESH from IMPORTOBJ.

use document::entities::mesh::{Mesh, MeshFace};
use document::types::Vector3;
use glam::{DMat4, DQuat, DVec3};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Parse a glTF or GLB file into one MESH per placed mesh. `dir` is the
/// folder external `.bin` buffers are read from (`None`: only embedded
/// buffers); `scale` takes the file's metres to drawing units.
/// Returns `None` if the file has no usable geometry.
pub fn parse_gltf(bytes: &[u8], dir: Option<&Path>, scale: f64) -> Option<Vec<Mesh>> {
    let (json, glb_bin) = split_glb(bytes).unwrap_or((bytes, None));
    let root: Value = serde_json::from_slice(json).ok()?;
    let buffers: Vec<Option<Vec<u8>>> = list(&root, "buffers")
        .iter()
        .map(|buffer| match buffer.get("uri").and_then(Value::as_str) {
            None => glb_bin.map(<[u8]>::to_vec),
            Some(uri) => read_uri(uri, dir),
        })
        .collect();
    let file = File { root: &root, buffers };

    // The default scene's root nodes, or every root node when no scene is named.
    let scenes = list(&root, "scenes");
    let scene = root.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
    let roots: Vec<usize> = match scenes.get(scene) {
        Some(scene) => indices(scene, "nodes"),
        None => {
            let nodes = list(&root, "nodes");
            let children: Vec<usize> = nodes.iter().flat_map(|n| indices(n, "children")).collect();
            (0..nodes.len()).filter(|n| !children.contains(n)).collect()
        }
    };
    // glTF is Y-up in metres; the drawing is Z-up: (x, y, z) → (x, -z, y).
    let up = DMat4::from_cols_array(&[
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    let mut meshes = Vec::new();
    for node in roots {
        file.place(node, DMat4::from_scale(DVec3::splat(scale)) * up, 0, &mut meshes);
    }
    (!meshes.is_empty()).then_some(meshes)
}

/// The JSON and binary chunks of a GLB container; `None` if it is not one.
fn split_glb(bytes: &[u8]) -> Option<(&[u8], Option<&[u8]>)> {
    let word = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize);
    if bytes.get(..4)? != b"glTF" {
        return None;
    }
    let (json_len, mut at) = (word(12)?, 20);
    let json = bytes.get(at..at + json_len)?;
    at += json_len;
    let bin = word(at)
        .filter(|_| bytes.get(at + 4..at + 8) == Some(&b"BIN\0"[..]))
        .and_then(|len| bytes.get(at + 8..at + 8 + len));
    Some((json, bin))
}

/// A buffer's bytes from a `data:` URI or a file next to the glTF.
fn read_uri(uri: &str, dir: Option<&Path>) -> Option<Vec<u8>> {
    use base64::Engine as _;
    if let Some(data) = uri.strip_prefix("data:") {
        let (_, encoded) = data.split_once(";base64,")?;
        return base64::engine::general_purpose::STANDARD.decode(encoded).ok();
    }
    let name = uri.replace("%20", " ");
    // Only a file beside the glTF or below it, never a path out of its folder.
    if name.contains("..") || Path::new(&name).is_absolute() {
        return None;
    }
    std::fs::read(dir?.join(name)).ok()
}

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn indices(value: &Value, key: &str) -> Vec<usize> {
    list(value, key).iter().filter_map(Value::as_u64).map(|n| n as usize).collect()
}

fn numbers<const N: usize>(value: &Value, key: &str) -> Option<[f64; N]> {
    let list = value.get(key)?.as_array()?;
    let mut out = [0.0; N];
    for (slot, n) in out.iter_mut().zip(list) {
        *slot = n.as_f64()?;
    }
    (list.len() == N).then_some(out)
}

struct File<'a> {
    root: &'a Value,
    buffers: Vec<Option<Vec<u8>>>,
}

impl File<'_> {
    fn place(&self, node: usize, parent: DMat4, depth: usize, out: &mut Vec<Mesh>) {
        // A node graph that loops is malformed; stop instead of recursing forever.
        let Some(value) = list(self.root, "nodes").get(node).filter(|_| depth < 64) else {
            return;
        };
        let local = match numbers::<16>(value, "matrix") {
            Some(m) => DMat4::from_cols_array(&m),
            None => {
                let [x, y, z] = numbers(value, "translation").unwrap_or([0.0; 3]);
                let [qx, qy, qz, qw] = numbers(value, "rotation").unwrap_or([0.0, 0.0, 0.0, 1.0]);
                let [sx, sy, sz] = numbers(value, "scale").unwrap_or([1.0; 3]);
                DMat4::from_scale_rotation_translation(
                    DVec3::new(sx, sy, sz),
                    DQuat::from_xyzw(qx, qy, qz, qw).normalize(),
                    DVec3::new(x, y, z),
                )
            }
        };
        let world = parent * local;
        if let Some(mesh) = value
            .get("mesh")
            .and_then(Value::as_u64)
            .and_then(|m| self.mesh(m as usize, world))
        {
            out.push(mesh);
        }
        for child in indices(value, "children") {
            self.place(child, world, depth + 1, out);
        }
    }

    /// Mesh `index`'s triangle primitives as one MESH placed by `world`.
    fn mesh(&self, index: usize, world: DMat4) -> Option<Mesh> {
        let value = list(self.root, "meshes").get(index)?;
        let mut mesh = Mesh::new();
        let mut joined: HashMap<[u64; 3], usize> = HashMap::new();
        for primitive in list(value, "primitives") {
            // ponytail: triangle lists only; strips, fans and Draco-compressed
            // primitives are skipped until a file needs them.
            if primitive.get("mode").and_then(Value::as_u64).unwrap_or(4) != 4
                || primitive.pointer("/extensions/KHR_draco_mesh_compression").is_some()
            {
                continue;
            }
            let Some(positions) = primitive
                .pointer("/attributes/POSITION")
                .and_then(Value::as_u64)
                .and_then(|a| self.accessor(a as usize, 3))
            else {
                continue;
            };
            let corners: Vec<usize> = match primitive.get("indices").and_then(Value::as_u64) {
                Some(a) => match self.accessor(a as usize, 1) {
                    Some(list) => list.iter().map(|i| i[0] as usize).collect(),
                    None => continue,
                },
                None => (0..positions.len()).collect(),
            };
            let mut vertex = |i: usize| -> Option<usize> {
                let p = positions.get(i)?;
                let w = world.transform_point3(DVec3::new(p[0], p[1], p[2]));
                Some(*joined
                    .entry([w.x, w.y, w.z].map(f64::to_bits))
                    .or_insert_with(|| mesh.add_vertex(Vector3::new(w.x, w.y, w.z))))
            };
            let faces: Vec<Vec<usize>> = corners
                .chunks_exact(3)
                .filter_map(|t| Some(vec![vertex(t[0])?, vertex(t[1])?, vertex(t[2])?]))
                .filter(|f| f[0] != f[1] && f[1] != f[2] && f[0] != f[2])
                .collect();
            for face in faces {
                mesh.add_face(MeshFace::new(face));
            }
        }
        if mesh.faces.is_empty() {
            return None;
        }
        mesh.compute_edges();
        Some(mesh)
    }

    /// An accessor's elements of `width` components each, as numbers
    /// (normalized integers scaled to 0..1 / -1..1).
    fn accessor(&self, index: usize, width: usize) -> Option<Vec<[f64; 3]>> {
        let accessor = list(self.root, "accessors").get(index)?;
        let kind = match accessor.get("type")?.as_str()? {
            "SCALAR" => 1,
            "VEC3" => 3,
            _ => return None,
        };
        if kind != width {
            return None;
        }
        let count = accessor.get("count")?.as_u64()? as usize;
        let view = list(self.root, "bufferViews").get(accessor.get("bufferView")?.as_u64()? as usize)?;
        let buffer = self.buffers.get(view.get("buffer")?.as_u64()? as usize)?.as_deref()?;
        let offset = |v: &Value| v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let start = offset(view) + offset(accessor);
        let normalized = accessor.get("normalized").and_then(Value::as_bool).unwrap_or(false);
        let (size, read): (usize, fn(&[u8]) -> f64) = match accessor.get("componentType")?.as_u64()? {
            5120 => (1, |b| b[0] as i8 as f64),
            5121 => (1, |b| b[0] as f64),
            5122 => (2, |b| i16::from_le_bytes([b[0], b[1]]) as f64),
            5123 => (2, |b| u16::from_le_bytes([b[0], b[1]]) as f64),
            5125 => (4, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64),
            5126 => (4, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64),
            _ => return None,
        };
        let range = match (normalized, size) {
            (false, _) | (_, 4) => 1.0,
            (true, 1) => 255.0,
            (true, _) => 65535.0,
        };
        let stride = view
            .get("byteStride")
            .and_then(Value::as_u64)
            .map_or(size * width, |s| s as usize);
        (0..count)
            .map(|n| {
                let at = start + n * stride;
                let mut out = [0.0; 3];
                for (k, slot) in out.iter_mut().take(width).enumerate() {
                    *slot = read(buffer.get(at + k * size..at + (k + 1) * size)?) / range;
                }
                Some(out)
            })
            .collect()
    }
}
