//! glTF 2.0 importer.
//!
//! Supports `.gltf` (JSON + separate `.bin`) and `.glb` (binary container)
//! files. Produces one [`Mesh`] entity per mesh primitive.
//!
//! # Example
//!
//! ```rust,ignore
//! use acadrust::io::import::gltf::GltfImporter;
//!
//! let doc = GltfImporter::from_file("model.glb")?.import()?;
//! ```

pub mod json_parser;

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::document::CadDocument;
use crate::entities::mesh::{Mesh, MeshFace};
use crate::entities::EntityType;
use crate::error::{DxfError, Result};
use crate::types::Vector3;

use super::color_mapping::{create_material_layer, rgb_to_color};
use super::ImportConfig;

use json_parser::JsonValue;

/// glTF file importer.
pub struct GltfImporter {
    data: Vec<u8>,
    base_dir: Option<PathBuf>,
    config: ImportConfig,
}

impl GltfImporter {
    /// Create an importer from a file path.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut file = fs::File::open(path).map_err(|e| {
            DxfError::ImportError(format!("Cannot open '{}': {}", path.display(), e))
        })?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(|e| {
            DxfError::ImportError(format!("Cannot read '{}': {}", path.display(), e))
        })?;
        Ok(Self {
            data,
            base_dir: path.parent().map(|p| p.to_path_buf()),
            config: ImportConfig::default(),
        })
    }

    /// Create an importer from an in-memory buffer.
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self {
            data,
            base_dir: None,
            config: ImportConfig::default(),
        }
    }

    /// Create an importer from a reader.
    pub fn from_reader<R: Read>(mut reader: R) -> Result<Self> {
        let mut data = Vec::new();
        reader
            .read_to_end(&mut data)
            .map_err(|e| DxfError::ImportError(format!("Cannot read glTF data: {}", e)))?;
        Ok(Self {
            data,
            base_dir: None,
            config: ImportConfig::default(),
        })
    }

    /// Set the import configuration.
    pub fn with_config(mut self, config: ImportConfig) -> Self {
        self.config = config;
        self
    }

    /// Perform the import and return a [`CadDocument`].
    pub fn import(&self) -> Result<CadDocument> {
        let (json, buffers) = self.parse()?;
        self.build_document(&json, &buffers)
    }

    /// Parse the glTF data into JSON and binary buffer(s).
    fn parse(&self) -> Result<(JsonValue, Vec<Vec<u8>>)> {
        if self.data.len() >= 4 && &self.data[0..4] == b"glTF" {
            self.parse_glb()
        } else {
            self.parse_gltf()
        }
    }

    /// Parse a GLB binary container.
    fn parse_glb(&self) -> Result<(JsonValue, Vec<Vec<u8>>)> {
        if self.data.len() < 12 {
            return Err(DxfError::ImportError("GLB file too short".to_string()));
        }

        let mut cursor = std::io::Cursor::new(&self.data);
        let _magic = cursor.read_u32::<LittleEndian>().unwrap();
        let version = cursor.read_u32::<LittleEndian>().unwrap();
        let _length = cursor.read_u32::<LittleEndian>().unwrap();

        if version != 2 {
            return Err(DxfError::ImportError(format!(
                "Unsupported GLB version: {} (only v2 supported)",
                version
            )));
        }

        let mut json_value = None;
        let mut bin_data: Vec<Vec<u8>> = Vec::new();

        while (cursor.position() as usize) + 8 <= self.data.len() {
            let chunk_length = cursor.read_u32::<LittleEndian>().unwrap() as usize;
            let chunk_type = cursor.read_u32::<LittleEndian>().unwrap();
            let pos = cursor.position() as usize;

            if pos + chunk_length > self.data.len() {
                break;
            }

            match chunk_type {
                0x4E4F534A => {
                    // "JSON"
                    let json_str = std::str::from_utf8(&self.data[pos..pos + chunk_length])
                        .map_err(|_| {
                            DxfError::ImportError("Invalid UTF-8 in GLB JSON chunk".to_string())
                        })?;
                    json_value = Some(JsonValue::parse(json_str)?);
                }
                0x004E4942 => {
                    // "BIN\0"
                    bin_data.push(self.data[pos..pos + chunk_length].to_vec());
                }
                _ => {} // Unknown chunk, skip
            }

            cursor.set_position((pos + chunk_length) as u64);
        }

        let json = json_value
            .ok_or_else(|| DxfError::ImportError("GLB missing JSON chunk".to_string()))?;

        Ok((json, bin_data))
    }

    /// Parse a .gltf text file + external buffer(s).
    fn parse_gltf(&self) -> Result<(JsonValue, Vec<Vec<u8>>)> {
        let text = String::from_utf8_lossy(&self.data);
        let json = JsonValue::parse(&text)?;

        let mut buffers = Vec::new();

        if let Some(buffer_descs) = json.get("buffers").and_then(|v| v.as_array()) {
            for buf_desc in buffer_descs {
                if let Some(uri) = buf_desc.get("uri").and_then(|v| v.as_str()) {
                    if let Some(data) = self.resolve_uri(uri)? {
                        buffers.push(data);
                    }
                }
                // If no URI, it's the GLB embedded buffer (shouldn't happen in .gltf)
            }
        }

        Ok((json, buffers))
    }

    /// Resolve a buffer URI — supports `data:` base64 and file paths.
    fn resolve_uri(&self, uri: &str) -> Result<Option<Vec<u8>>> {
        if uri.starts_with("data:") {
            // data URI: data:application/octet-stream;base64,AAAA...
            if let Some(comma_pos) = uri.find(',') {
                let encoded = &uri[comma_pos + 1..];
                let decoded = base64_decode(encoded)?;
                return Ok(Some(decoded));
            }
            return Ok(None);
        }

        // File path relative to .gltf
        if let Some(base) = &self.base_dir {
            let path = base.join(uri);
            match fs::read(&path) {
                Ok(data) => return Ok(Some(data)),
                Err(e) => {
                    return Err(DxfError::ImportError(format!(
                        "Cannot read buffer '{}': {}",
                        path.display(),
                        e
                    )));
                }
            }
        }

        Ok(None)
    }

    fn build_document(&self, json: &JsonValue, buffers: &[Vec<u8>]) -> Result<CadDocument> {
        let mut doc = CadDocument::new();
        let scale = self.config.scale_factor;

        let accessors = json.get("accessors").and_then(|v| v.as_array());
        let buffer_views = json.get("bufferViews").and_then(|v| v.as_array());
        let materials_arr = json.get("materials").and_then(|v| v.as_array());
        let meshes = json.get("meshes").and_then(|v| v.as_array());

        let meshes = match meshes {
            Some(m) => m,
            None => return Ok(doc), // No meshes
        };

        for gltf_mesh in meshes {
            let mesh_name = gltf_mesh
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("mesh");

            let primitives = match gltf_mesh.get("primitives").and_then(|v| v.as_array()) {
                Some(p) => p,
                None => continue,
            };

            for prim in primitives {
                let attributes = match prim.get("attributes") {
                    Some(a) => a,
                    None => continue,
                };

                // Get POSITION accessor index
                let pos_accessor_idx = match attributes.get("POSITION").and_then(|v| v.as_usize()) {
                    Some(i) => i,
                    None => continue,
                };

                // Read positions
                let positions =
                    read_accessor_vec3(pos_accessor_idx, accessors, buffer_views, buffers)?;

                if positions.is_empty() {
                    continue;
                }

                // Read indices (optional)
                let indices = if let Some(idx) = prim.get("indices").and_then(|v| v.as_usize()) {
                    read_accessor_scalar(idx, accessors, buffer_views, buffers)?
                } else {
                    // Non-indexed: sequential triangles
                    (0..positions.len()).collect()
                };

                // Get material color
                let (mat_name, color) =
                    if let Some(mat_idx) = prim.get("material").and_then(|v| v.as_usize()) {
                        get_material_color(mat_idx, materials_arr)
                    } else {
                        (String::new(), [0.8f32, 0.8, 0.8, 1.0])
                    };

                // Build mesh
                let mesh = build_gltf_mesh(
                    &positions,
                    &indices,
                    scale,
                    self.config.merge_vertices,
                    self.config.merge_tolerance,
                );

                if mesh.vertices.is_empty() {
                    continue;
                }

                let layer_label = if mat_name.is_empty() {
                    mesh_name.to_string()
                } else {
                    mat_name.clone()
                };

                let c = if mat_name.is_empty() {
                    self.config.default_color
                } else {
                    rgb_to_color(
                        (color[0] * 255.0) as u8,
                        (color[1] * 255.0) as u8,
                        (color[2] * 255.0) as u8,
                        true,
                    )
                };

                let layer =
                    create_material_layer(&mut doc, &self.config.layer_prefix, &layer_label, c);

                let mut mesh_entity = mesh;
                mesh_entity.common.layer = layer;
                mesh_entity.common.color = c;
                doc.add_entity(EntityType::Mesh(mesh_entity))?;
            }
        }

        Ok(doc)
    }
}

/// Read Vec3 positions from an accessor.
fn read_accessor_vec3(
    accessor_idx: usize,
    accessors: Option<&[JsonValue]>,
    buffer_views: Option<&[JsonValue]>,
    buffers: &[Vec<u8>],
) -> Result<Vec<[f32; 3]>> {
    let accessors =
        accessors.ok_or_else(|| DxfError::ImportError("Missing accessors array".to_string()))?;
    let accessor = accessors
        .get(accessor_idx)
        .ok_or_else(|| DxfError::ImportError(format!("Accessor {} out of range", accessor_idx)))?;

    let count = accessor
        .get("count")
        .and_then(|v| v.as_usize())
        .unwrap_or(0);
    let component_type = accessor
        .get("componentType")
        .and_then(|v| v.as_usize())
        .unwrap_or(0);
    let bv_idx = accessor.get("bufferView").and_then(|v| v.as_usize());
    let byte_offset_acc = accessor
        .get("byteOffset")
        .and_then(|v| v.as_usize())
        .unwrap_or(0);

    if component_type != 5126 {
        // 5126 = FLOAT
        return Err(DxfError::ImportError(format!(
            "Unsupported POSITION component type: {} (expected FLOAT/5126)",
            component_type
        )));
    }

    let bv_idx = match bv_idx {
        Some(i) => i,
        None => return Ok(Vec::new()),
    };

    let buffer_views = buffer_views
        .ok_or_else(|| DxfError::ImportError("Missing bufferViews array".to_string()))?;
    let bv = buffer_views
        .get(bv_idx)
        .ok_or_else(|| DxfError::ImportError(format!("BufferView {} out of range", bv_idx)))?;

    let buffer_idx = bv.get("buffer").and_then(|v| v.as_usize()).unwrap_or(0);
    let byte_offset_bv = bv.get("byteOffset").and_then(|v| v.as_usize()).unwrap_or(0);
    let byte_stride = bv
        .get("byteStride")
        .and_then(|v| v.as_usize())
        .unwrap_or(12); // 3 floats

    let buffer = buffers
        .get(buffer_idx)
        .ok_or_else(|| DxfError::ImportError(format!("Buffer {} not loaded", buffer_idx)))?;

    let start = byte_offset_bv + byte_offset_acc;
    let mut positions = Vec::with_capacity(count);

    for i in 0..count {
        let offset = start + i * byte_stride;
        if offset + 12 > buffer.len() {
            break;
        }
        let x = f32::from_le_bytes([
            buffer[offset],
            buffer[offset + 1],
            buffer[offset + 2],
            buffer[offset + 3],
        ]);
        let y = f32::from_le_bytes([
            buffer[offset + 4],
            buffer[offset + 5],
            buffer[offset + 6],
            buffer[offset + 7],
        ]);
        let z = f32::from_le_bytes([
            buffer[offset + 8],
            buffer[offset + 9],
            buffer[offset + 10],
            buffer[offset + 11],
        ]);
        positions.push([x, y, z]);
    }

    Ok(positions)
}

/// Read scalar indices from an accessor.
fn read_accessor_scalar(
    accessor_idx: usize,
    accessors: Option<&[JsonValue]>,
    buffer_views: Option<&[JsonValue]>,
    buffers: &[Vec<u8>],
) -> Result<Vec<usize>> {
    let accessors =
        accessors.ok_or_else(|| DxfError::ImportError("Missing accessors array".to_string()))?;
    let accessor = accessors
        .get(accessor_idx)
        .ok_or_else(|| DxfError::ImportError(format!("Accessor {} out of range", accessor_idx)))?;

    let count = accessor
        .get("count")
        .and_then(|v| v.as_usize())
        .unwrap_or(0);
    let component_type = accessor
        .get("componentType")
        .and_then(|v| v.as_usize())
        .unwrap_or(0);
    let bv_idx = accessor.get("bufferView").and_then(|v| v.as_usize());
    let byte_offset_acc = accessor
        .get("byteOffset")
        .and_then(|v| v.as_usize())
        .unwrap_or(0);

    let bv_idx = match bv_idx {
        Some(i) => i,
        None => return Ok(Vec::new()),
    };

    let buffer_views = buffer_views
        .ok_or_else(|| DxfError::ImportError("Missing bufferViews array".to_string()))?;
    let bv = buffer_views
        .get(bv_idx)
        .ok_or_else(|| DxfError::ImportError(format!("BufferView {} out of range", bv_idx)))?;

    let buffer_idx = bv.get("buffer").and_then(|v| v.as_usize()).unwrap_or(0);
    let byte_offset_bv = bv.get("byteOffset").and_then(|v| v.as_usize()).unwrap_or(0);

    let buffer = buffers
        .get(buffer_idx)
        .ok_or_else(|| DxfError::ImportError(format!("Buffer {} not loaded", buffer_idx)))?;

    let start = byte_offset_bv + byte_offset_acc;
    let mut indices = Vec::with_capacity(count);

    match component_type {
        5121 => {
            // UNSIGNED_BYTE
            for i in 0..count {
                let offset = start + i;
                if offset >= buffer.len() {
                    break;
                }
                indices.push(buffer[offset] as usize);
            }
        }
        5123 => {
            // UNSIGNED_SHORT
            for i in 0..count {
                let offset = start + i * 2;
                if offset + 2 > buffer.len() {
                    break;
                }
                let v = u16::from_le_bytes([buffer[offset], buffer[offset + 1]]);
                indices.push(v as usize);
            }
        }
        5125 => {
            // UNSIGNED_INT
            for i in 0..count {
                let offset = start + i * 4;
                if offset + 4 > buffer.len() {
                    break;
                }
                let v = u32::from_le_bytes([
                    buffer[offset],
                    buffer[offset + 1],
                    buffer[offset + 2],
                    buffer[offset + 3],
                ]);
                indices.push(v as usize);
            }
        }
        _ => {
            return Err(DxfError::ImportError(format!(
                "Unsupported index component type: {}",
                component_type
            )));
        }
    }

    Ok(indices)
}

/// Extract material name and base color from a glTF material.
fn get_material_color(mat_idx: usize, materials: Option<&[JsonValue]>) -> (String, [f32; 4]) {
    let default = (String::new(), [0.8f32, 0.8, 0.8, 1.0]);

    let materials = match materials {
        Some(m) => m,
        None => return default,
    };
    let mat = match materials.get(mat_idx) {
        Some(m) => m,
        None => return default,
    };

    let name = mat
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // PBR metallic-roughness base color
    let mut color = [0.8f32, 0.8, 0.8, 1.0];
    if let Some(pbr) = mat.get("pbrMetallicRoughness") {
        if let Some(bc) = pbr.get("baseColorFactor").and_then(|v| v.as_array()) {
            if bc.len() >= 3 {
                color[0] = bc[0].as_f64().unwrap_or(0.8) as f32;
                color[1] = bc[1].as_f64().unwrap_or(0.8) as f32;
                color[2] = bc[2].as_f64().unwrap_or(0.8) as f32;
                if bc.len() >= 4 {
                    color[3] = bc[3].as_f64().unwrap_or(1.0) as f32;
                }
            }
        }
    }

    (name, color)
}

/// Build a Mesh from glTF positions and triangle indices.
fn build_gltf_mesh(
    positions: &[[f32; 3]],
    indices: &[usize],
    scale: f64,
    merge: bool,
    tolerance: f64,
) -> Mesh {
    let num_tris = indices.len() / 3;
    if num_tris == 0 {
        return Mesh::new();
    }

    if merge {
        let inv_tol = if tolerance > 0.0 {
            1.0 / tolerance
        } else {
            1e9
        };
        let mut mesh_verts: Vec<Vector3> = Vec::new();
        let mut vert_map: HashMap<(i64, i64, i64), usize> = HashMap::new();
        let mut faces = Vec::with_capacity(num_tris);

        let mut get_or_insert = |p: [f32; 3]| -> usize {
            let x = p[0] as f64 * scale;
            let y = p[1] as f64 * scale;
            let z = p[2] as f64 * scale;
            let key = (
                (x * inv_tol).round() as i64,
                (y * inv_tol).round() as i64,
                (z * inv_tol).round() as i64,
            );
            if let Some(&idx) = vert_map.get(&key) {
                idx
            } else {
                let idx = mesh_verts.len();
                mesh_verts.push(Vector3::new(x, y, z));
                vert_map.insert(key, idx);
                idx
            }
        };

        for tri in indices.chunks_exact(3) {
            let p0 = positions.get(tri[0]).copied().unwrap_or([0.0; 3]);
            let p1 = positions.get(tri[1]).copied().unwrap_or([0.0; 3]);
            let p2 = positions.get(tri[2]).copied().unwrap_or([0.0; 3]);
            let i0 = get_or_insert(p0);
            let i1 = get_or_insert(p1);
            let i2 = get_or_insert(p2);
            if i0 != i1 && i1 != i2 && i0 != i2 {
                faces.push(MeshFace::triangle(i0, i1, i2));
            }
        }

        let mut mesh = Mesh::new();
        mesh.vertices = mesh_verts;
        mesh.faces = faces;
        mesh.compute_edges();
        mesh
    } else {
        let mut verts = Vec::with_capacity(num_tris * 3);
        let mut faces = Vec::with_capacity(num_tris);

        for tri in indices.chunks_exact(3) {
            let base = verts.len();
            for &idx in tri {
                let p = positions.get(idx).copied().unwrap_or([0.0; 3]);
                verts.push(Vector3::new(
                    p[0] as f64 * scale,
                    p[1] as f64 * scale,
                    p[2] as f64 * scale,
                ));
            }
            faces.push(MeshFace::triangle(base, base + 1, base + 2));
        }

        let mut mesh = Mesh::new();
        mesh.vertices = verts;
        mesh.faces = faces;
        mesh.compute_edges();
        mesh
    }
}

/// Simple base64 decoder (standard alphabet, no padding required).
fn base64_decode(input: &str) -> Result<Vec<u8>> {
    const DECODE: [u8; 128] = {
        let mut t = [255u8; 128];
        let mut i = 0u8;
        while i < 26 {
            t[(b'A' + i) as usize] = i;
            t[(b'a' + i) as usize] = i + 26;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            t[(b'0' + i) as usize] = i + 52;
            i += 1;
        }
        t[b'+' as usize] = 62;
        t[b'/' as usize] = 63;
        t
    };

    let bytes: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'=' && b != b'\n' && b != b'\r' && b != b' ')
        .collect();

    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);
    let chunks = bytes.chunks(4);

    for chunk in chunks {
        let mut buf = [0u32; 4];
        for (i, &b) in chunk.iter().enumerate() {
            if b >= 128 || DECODE[b as usize] == 255 {
                return Err(DxfError::ImportError(
                    "Invalid base64 character".to_string(),
                ));
            }
            buf[i] = DECODE[b as usize] as u32;
        }
        let n = (buf[0] << 18) | (buf[1] << 12) | (buf[2] << 6) | buf[3];
        output.push((n >> 16) as u8);
        if chunk.len() > 2 {
            output.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            output.push(n as u8);
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_decode() {
        let decoded = base64_decode("SGVsbG8=").unwrap();
        assert_eq!(&decoded, b"Hello");
    }

    #[test]
    fn test_glb_magic() {
        let importer = GltfImporter::from_bytes(b"not glb".to_vec());
        // Should parse as gltf text — will fail JSON parse
        assert!(importer.import().is_err());
    }

    #[test]
    fn test_import_minimal_gltf() {
        // Minimal glTF with embedded base64 buffer:
        // 1 triangle, 3 vertices, 3 indices
        let positions: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let indices: [u16; 3] = [0, 1, 2];

        let mut buf = Vec::new();
        for p in &positions {
            buf.extend_from_slice(&p.to_le_bytes());
        }
        for i in &indices {
            buf.extend_from_slice(&i.to_le_bytes());
        }
        // Pad to 4 bytes
        while buf.len() % 4 != 0 {
            buf.push(0);
        }

        let b64 = base64_encode(&buf);
        let uri = format!("data:application/octet-stream;base64,{}", b64);

        let json = format!(
            r#"{{
  "asset": {{"version": "2.0"}},
  "buffers": [{{"uri": "{uri}", "byteLength": {}}}],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 36}},
    {{"buffer": 0, "byteOffset": 36, "byteLength": 6}}
  ],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "max": [1,1,0], "min": [0,0,0]}},
    {{"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}}
  ],
  "meshes": [{{"name": "Triangle", "primitives": [{{"attributes": {{"POSITION": 0}}, "indices": 1}}]}}]
}}"#,
            buf.len()
        );

        let importer = GltfImporter::from_bytes(json.as_bytes().to_vec());
        let doc = importer.import().unwrap();
        let entities: Vec<_> = doc.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::Mesh(mesh) = &entities[0] {
            assert_eq!(mesh.vertices.len(), 3);
            assert_eq!(mesh.faces.len(), 1);
        } else {
            panic!("Expected Mesh");
        }
    }

    /// Simple base64 encoder for tests.
    fn base64_encode(data: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[(n >> 18 & 63) as usize] as char);
            out.push(TABLE[(n >> 12 & 63) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[(n >> 6 & 63) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(TABLE[(n & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }
}
