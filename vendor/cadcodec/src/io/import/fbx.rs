//! FBX (Filmbox) importer.
//!
//! Supports FBX binary format (versions 7100–7700, i.e. FBX 2011–2020+) and
//! ASCII FBX.  Produces one [`Mesh`] entity per geometry node.
//!
//! # Example
//!
//! ```rust,ignore
//! use acadrust::io::import::fbx::FbxImporter;
//!
//! let doc = FbxImporter::from_file("model.fbx")?.import()?;
//! ```

use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use byteorder::{LittleEndian, ReadBytesExt};

use crate::document::CadDocument;
use crate::entities::mesh::{Mesh, MeshFace};
use crate::entities::EntityType;
use crate::error::{DxfError, Result};
use crate::types::Vector3;

use super::color_mapping::{create_material_layer, rgb_to_color};
use super::ImportConfig;

// ─── FBX node tree ───────────────────────────────────────────────────────

/// An FBX property value.
#[derive(Debug, Clone)]
enum FbxProp {
    Ignored,
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Str(String),
    ArrayI32(Vec<i32>),
    ArrayF64(Vec<f64>),
}

/// An FBX node.
#[derive(Debug, Clone)]
struct FbxNode {
    name: String,
    properties: Vec<FbxProp>,
    children: Vec<FbxNode>,
}

impl FbxNode {
    fn child(&self, name: &str) -> Option<&FbxNode> {
        self.children.iter().find(|c| c.name == name)
    }

    fn children_named(&self, name: &str) -> Vec<&FbxNode> {
        self.children.iter().filter(|c| c.name == name).collect()
    }

    fn prop_str(&self, index: usize) -> Option<&str> {
        self.properties.get(index).and_then(|p| match p {
            FbxProp::Str(s) => Some(s.as_str()),
            _ => None,
        })
    }

    fn prop_i64(&self, index: usize) -> Option<i64> {
        self.properties.get(index).and_then(|p| match p {
            FbxProp::I64(v) => Some(*v),
            FbxProp::I32(v) => Some(*v as i64),
            FbxProp::I16(v) => Some(*v as i64),
            _ => None,
        })
    }

    fn prop_f64_array(&self, index: usize) -> Option<&Vec<f64>> {
        self.properties.get(index).and_then(|p| match p {
            FbxProp::ArrayF64(v) => Some(v),
            _ => None,
        })
    }

    fn prop_i32_array(&self, index: usize) -> Option<&Vec<i32>> {
        self.properties.get(index).and_then(|p| match p {
            FbxProp::ArrayI32(v) => Some(v),
            _ => None,
        })
    }
}

// ─── Extracted geometry ──────────────────────────────────────────────────

#[derive(Debug)]
struct FbxGeometry {
    id: i64,
    name: String,
    vertices: Vec<[f64; 3]>,
    polygon_indices: Vec<i32>,
}

#[derive(Debug)]
struct FbxMaterialInfo {
    id: i64,
    name: String,
    diffuse: [f32; 3],
}

// ─── FBX Importer ────────────────────────────────────────────────────────

/// FBX file importer.
pub struct FbxImporter {
    data: Vec<u8>,
    config: ImportConfig,
}

impl FbxImporter {
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
            config: ImportConfig::default(),
        })
    }

    /// Create an importer from an in-memory buffer.
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self {
            data,
            config: ImportConfig::default(),
        }
    }

    /// Create an importer from a reader.
    pub fn from_reader<R: Read>(mut reader: R) -> Result<Self> {
        let mut data = Vec::new();
        reader
            .read_to_end(&mut data)
            .map_err(|e| DxfError::ImportError(format!("Cannot read FBX data: {}", e)))?;
        Ok(Self {
            data,
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
        if is_binary_fbx(&self.data) {
            let nodes = parse_binary_fbx(&self.data)?;
            self.build_document(&nodes)
        } else {
            let nodes = parse_ascii_fbx(&self.data)?;
            self.build_document(&nodes)
        }
    }

    fn build_document(&self, nodes: &[FbxNode]) -> Result<CadDocument> {
        let mut doc = CadDocument::new();
        let scale = self.config.scale_factor;

        // Find the Objects node
        let objects = match nodes.iter().find(|n| n.name == "Objects") {
            Some(o) => o,
            None => return Ok(doc), // No objects
        };

        // Extract geometries
        let mut geometries: HashMap<i64, FbxGeometry> = HashMap::new();
        for node in objects.children_named("Geometry") {
            if let Some(geom) = extract_geometry(node) {
                geometries.insert(geom.id, geom);
            }
        }

        // Extract materials
        let mut materials: HashMap<i64, FbxMaterialInfo> = HashMap::new();
        for node in objects.children_named("Material") {
            if let Some(mat) = extract_material(node) {
                materials.insert(mat.id, mat);
            }
        }

        // Extract model names
        let mut model_names: HashMap<i64, String> = HashMap::new();
        for node in objects.children_named("Model") {
            if let Some(id) = node.prop_i64(0) {
                let name = node.prop_str(1).unwrap_or("Model").to_string();
                // FBX names often have "Model::" prefix
                let clean = name.rsplit("::").next().unwrap_or(&name).trim().to_string();
                model_names.insert(id, clean);
            }
        }

        // Build connections map: child_id → parent_id
        let connections = nodes.iter().find(|n| n.name == "Connections");
        let mut geom_to_model: HashMap<i64, i64> = HashMap::new();
        let mut mat_to_model: HashMap<i64, Vec<i64>> = HashMap::new();

        if let Some(conn_node) = connections {
            for c in conn_node.children_named("C") {
                if c.properties.len() >= 3 {
                    if let (Some(child_id), Some(parent_id)) = (c.prop_i64(1), c.prop_i64(2)) {
                        // Check if child is a geometry
                        if geometries.contains_key(&child_id) {
                            geom_to_model.insert(child_id, parent_id);
                        }
                        // Check if child is a material
                        if materials.contains_key(&child_id) {
                            mat_to_model.entry(parent_id).or_default().push(child_id);
                        }
                    }
                }
            }
        }

        // Build meshes
        for (geom_id, geom) in &geometries {
            let model_id = geom_to_model.get(geom_id).copied();
            let model_name = model_id
                .and_then(|id| model_names.get(&id))
                .cloned()
                .unwrap_or_else(|| geom.name.clone());

            // Get material for this model
            let mat_info = model_id
                .and_then(|mid| mat_to_model.get(&mid))
                .and_then(|mats| mats.first())
                .and_then(|mat_id| materials.get(mat_id));

            let (layer_label, color) = if let Some(mat) = mat_info {
                let c = rgb_to_color(
                    (mat.diffuse[0] * 255.0) as u8,
                    (mat.diffuse[1] * 255.0) as u8,
                    (mat.diffuse[2] * 255.0) as u8,
                    true,
                );
                (mat.name.clone(), c)
            } else {
                (model_name.clone(), self.config.default_color)
            };

            let mesh = build_fbx_mesh(
                &geom.vertices,
                &geom.polygon_indices,
                scale,
                self.config.merge_vertices,
                self.config.merge_tolerance,
            );

            if mesh.vertices.is_empty() {
                continue;
            }

            let layer =
                create_material_layer(&mut doc, &self.config.layer_prefix, &layer_label, color);

            let mut mesh_entity = mesh;
            mesh_entity.common.layer = layer;
            mesh_entity.common.color = color;
            doc.add_entity(EntityType::Mesh(mesh_entity))?;
        }

        Ok(doc)
    }
}

// ─── Binary FBX parser ───────────────────────────────────────────────────

const FBX_MAGIC: &[u8; 21] = b"Kaydara FBX Binary  \0";

fn is_binary_fbx(data: &[u8]) -> bool {
    data.len() >= 27 && data[..21] == *FBX_MAGIC
}

fn parse_binary_fbx(data: &[u8]) -> Result<Vec<FbxNode>> {
    if !is_binary_fbx(data) {
        return Err(DxfError::ImportError("Not a binary FBX file".to_string()));
    }

    let mut cursor = Cursor::new(data);
    cursor.set_position(21);
    // Skip 2 padding bytes
    cursor.set_position(23);
    let version = cursor
        .read_u32::<LittleEndian>()
        .map_err(|e| DxfError::ImportError(format!("Cannot read FBX version: {}", e)))?;

    // Version >= 7500 uses 64-bit node offsets
    let use_64bit = version >= 7500;

    let mut nodes = Vec::new();
    let mut pos = 27u64;

    loop {
        cursor.set_position(pos);
        let node = read_fbx_node(data, &mut cursor, use_64bit)?;
        match node {
            None => break, // NULL record
            Some(n) => {
                pos = cursor.position();
                nodes.push(n);
            }
        }
    }

    Ok(nodes)
}

fn read_fbx_node(
    data: &[u8],
    cursor: &mut Cursor<&[u8]>,
    use_64bit: bool,
) -> Result<Option<FbxNode>> {
    let (end_offset, num_properties, property_list_len, name_len) = if use_64bit {
        let end = cursor.read_u64::<LittleEndian>().unwrap_or(0);
        let np = cursor.read_u64::<LittleEndian>().unwrap_or(0);
        let pl = cursor.read_u64::<LittleEndian>().unwrap_or(0);
        let nl = cursor.read_u8().unwrap_or(0);
        (end, np, pl, nl)
    } else {
        let end = cursor.read_u32::<LittleEndian>().unwrap_or(0) as u64;
        let np = cursor.read_u32::<LittleEndian>().unwrap_or(0) as u64;
        let pl = cursor.read_u32::<LittleEndian>().unwrap_or(0) as u64;
        let nl = cursor.read_u8().unwrap_or(0);
        (end, np, pl, nl)
    };

    // NULL record — marks end of children
    if end_offset == 0 && num_properties == 0 && property_list_len == 0 && name_len == 0 {
        return Ok(None);
    }

    let pos = cursor.position() as usize;
    if pos + name_len as usize > data.len() {
        return Err(DxfError::ImportError(
            "FBX node name out of bounds".to_string(),
        ));
    }
    let name = String::from_utf8_lossy(&data[pos..pos + name_len as usize]).to_string();
    cursor.set_position((pos + name_len as usize) as u64);

    // Read properties
    let mut properties = Vec::new();
    let prop_start = cursor.position();
    for _ in 0..num_properties {
        if cursor.position() >= prop_start + property_list_len {
            break;
        }
        match read_fbx_property(data, cursor) {
            Ok(prop) => properties.push(prop),
            Err(_) => break,
        }
    }

    // Skip to after properties
    cursor.set_position(prop_start + property_list_len);

    // Read children
    let mut children = Vec::new();
    if end_offset > 0 && (cursor.position() as u64) < end_offset {
        while (cursor.position() as u64) < end_offset {
            match read_fbx_node(data, cursor, use_64bit)? {
                None => break, // NULL terminator
                Some(child) => children.push(child),
            }
        }
    }

    cursor.set_position(end_offset);

    Ok(Some(FbxNode {
        name,
        properties,
        children,
    }))
}

fn read_fbx_property(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<FbxProp> {
    let type_code = cursor
        .read_u8()
        .map_err(|e| DxfError::ImportError(format!("Cannot read FBX property type: {}", e)))?;

    match type_code {
        b'C' => {
            let _ = cursor.read_u8();
            Ok(FbxProp::Ignored)
        }
        b'Y' => {
            let v = cursor.read_i16::<LittleEndian>().unwrap_or(0);
            Ok(FbxProp::I16(v))
        }
        b'I' => {
            let v = cursor.read_i32::<LittleEndian>().unwrap_or(0);
            Ok(FbxProp::I32(v))
        }
        b'L' => {
            let v = cursor.read_i64::<LittleEndian>().unwrap_or(0);
            Ok(FbxProp::I64(v))
        }
        b'F' => {
            let v = cursor.read_f32::<LittleEndian>().unwrap_or(0.0);
            Ok(FbxProp::F32(v))
        }
        b'D' => {
            let v = cursor.read_f64::<LittleEndian>().unwrap_or(0.0);
            Ok(FbxProp::F64(v))
        }
        b'S' | b'R' => {
            let len = cursor.read_u32::<LittleEndian>().unwrap_or(0) as usize;
            let pos = cursor.position() as usize;
            if pos + len > data.len() {
                return Err(DxfError::ImportError(
                    "FBX string out of bounds".to_string(),
                ));
            }
            let bytes = &data[pos..pos + len];
            cursor.set_position((pos + len) as u64);
            if type_code == b'S' {
                Ok(FbxProp::Str(String::from_utf8_lossy(bytes).to_string()))
            } else {
                Ok(FbxProp::Ignored)
            }
        }
        b'i' => read_array_i32(data, cursor),
        b'l' => read_array_i64(data, cursor),
        b'f' => read_array_f32(data, cursor),
        b'd' => read_array_f64(data, cursor),
        b'b' => read_array_bool(data, cursor),
        _ => Err(DxfError::ImportError(format!(
            "Unknown FBX property type: '{}'",
            type_code as char
        ))),
    }
}

fn read_array_header(cursor: &mut Cursor<&[u8]>) -> Result<(u32, u32, u32)> {
    let count = cursor.read_u32::<LittleEndian>().unwrap_or(0);
    let encoding = cursor.read_u32::<LittleEndian>().unwrap_or(0);
    let compressed_len = cursor.read_u32::<LittleEndian>().unwrap_or(0);
    Ok((count, encoding, compressed_len))
}

fn decompress_array_data(
    data: &[u8],
    cursor: &mut Cursor<&[u8]>,
    encoding: u32,
    compressed_len: u32,
    element_size: usize,
    count: u32,
) -> Result<Vec<u8>> {
    let pos = cursor.position() as usize;
    if encoding == 1 {
        // zlib/deflate compressed
        if pos + compressed_len as usize > data.len() {
            return Err(DxfError::ImportError(
                "FBX compressed array out of bounds".to_string(),
            ));
        }
        let compressed = &data[pos..pos + compressed_len as usize];
        cursor.set_position((pos + compressed_len as usize) as u64);

        let mut decoder = flate2::read::ZlibDecoder::new(compressed);
        let expected = count as usize * element_size;
        let mut decompressed = vec![0u8; expected];
        decoder
            .read_exact(&mut decompressed)
            .map_err(|e| DxfError::ImportError(format!("FBX array decompression error: {}", e)))?;
        Ok(decompressed)
    } else {
        // Uncompressed
        let byte_len = count as usize * element_size;
        if pos + byte_len > data.len() {
            return Err(DxfError::ImportError("FBX array out of bounds".to_string()));
        }
        let result = data[pos..pos + byte_len].to_vec();
        cursor.set_position((pos + byte_len) as u64);
        Ok(result)
    }
}

fn read_array_f64(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<FbxProp> {
    let (count, encoding, compressed_len) = read_array_header(cursor)?;
    let raw = decompress_array_data(data, cursor, encoding, compressed_len, 8, count)?;
    let mut values = Vec::with_capacity(count as usize);
    let mut c = Cursor::new(&raw[..]);
    for _ in 0..count {
        values.push(c.read_f64::<LittleEndian>().unwrap_or(0.0));
    }
    Ok(FbxProp::ArrayF64(values))
}

fn read_array_f32(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<FbxProp> {
    let (count, encoding, compressed_len) = read_array_header(cursor)?;
    let _ = decompress_array_data(data, cursor, encoding, compressed_len, 4, count)?;
    Ok(FbxProp::Ignored)
}

fn read_array_i32(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<FbxProp> {
    let (count, encoding, compressed_len) = read_array_header(cursor)?;
    let raw = decompress_array_data(data, cursor, encoding, compressed_len, 4, count)?;
    let mut values = Vec::with_capacity(count as usize);
    let mut c = Cursor::new(&raw[..]);
    for _ in 0..count {
        values.push(c.read_i32::<LittleEndian>().unwrap_or(0));
    }
    Ok(FbxProp::ArrayI32(values))
}

fn read_array_i64(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<FbxProp> {
    let (count, encoding, compressed_len) = read_array_header(cursor)?;
    let _ = decompress_array_data(data, cursor, encoding, compressed_len, 8, count)?;
    Ok(FbxProp::Ignored)
}

fn read_array_bool(data: &[u8], cursor: &mut Cursor<&[u8]>) -> Result<FbxProp> {
    let (count, encoding, compressed_len) = read_array_header(cursor)?;
    let _ = decompress_array_data(data, cursor, encoding, compressed_len, 1, count)?;
    Ok(FbxProp::Ignored)
}

// ─── ASCII FBX parser ────────────────────────────────────────────────────

fn parse_ascii_fbx(data: &[u8]) -> Result<Vec<FbxNode>> {
    let text = String::from_utf8_lossy(data);
    let mut chars = text.chars().peekable();
    let mut nodes = Vec::new();

    loop {
        skip_ws_and_comments(&mut chars);
        if chars.peek().is_none() {
            break;
        }
        if let Some(node) = parse_ascii_node(&mut chars)? {
            nodes.push(node);
        } else {
            break;
        }
    }

    Ok(nodes)
}

fn skip_ws_and_comments(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    loop {
        match chars.peek() {
            Some(&c) if c.is_whitespace() => {
                chars.next();
            }
            Some(&';') => {
                // Line comment
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => break,
        }
    }
}

fn parse_ascii_node(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<Option<FbxNode>> {
    skip_ws_and_comments(chars);

    // Read node name (identifier followed by ':')
    let mut name = String::new();
    loop {
        match chars.peek() {
            Some(&c) if c.is_alphanumeric() || c == '_' => {
                name.push(c);
                chars.next();
            }
            Some(&':') => {
                chars.next();
                break;
            }
            Some(&'}') => return Ok(None), // End of parent
            None => return Ok(None),
            Some(&c) => {
                return Err(DxfError::ImportError(format!(
                    "Unexpected character '{}' in FBX node name",
                    c
                )));
            }
        }
    }

    // Read properties until '{' or newline
    let mut properties = Vec::new();
    skip_ws_no_newline(chars);

    loop {
        match chars.peek() {
            Some(&'{') => {
                chars.next();
                break;
            }
            Some(&'\n') | Some(&'\r') => {
                chars.next();
                // Check if next non-whitespace is '{'
                skip_ws_and_comments(chars);
                if chars.peek() == Some(&'{') {
                    chars.next();
                    break;
                }
                // No children block
                return Ok(Some(FbxNode {
                    name,
                    properties,
                    children: Vec::new(),
                }));
            }
            Some(&',') => {
                chars.next();
                skip_ws_no_newline(chars);
            }
            Some(&'"') => {
                let s = parse_ascii_string(chars)?;
                properties.push(FbxProp::Str(s));
            }
            Some(&c) if c == '-' || c == '+' || c.is_ascii_digit() => {
                let num_str = parse_ascii_number_str(chars);
                if num_str.contains('.') || num_str.contains('e') || num_str.contains('E') {
                    if let Ok(v) = num_str.parse::<f64>() {
                        properties.push(FbxProp::F64(v));
                    }
                } else if let Ok(v) = num_str.parse::<i64>() {
                    properties.push(FbxProp::I64(v));
                }
            }
            Some(&'*') => {
                // Array: *count { a: v1, v2, ... }
                chars.next();
                let count_str = parse_ascii_number_str(chars);
                let _count: usize = count_str.parse().unwrap_or(0);
                skip_ws_and_comments(chars);
                if chars.peek() == Some(&'{') {
                    chars.next();
                    skip_ws_and_comments(chars);
                    // Read "a:" prefix
                    let mut prefix = String::new();
                    loop {
                        match chars.peek() {
                            Some(&':') => {
                                chars.next();
                                break;
                            }
                            Some(&c) if c.is_alphanumeric() => {
                                prefix.push(c);
                                chars.next();
                            }
                            _ => break,
                        }
                    }
                    // Read values separated by commas
                    let values = parse_ascii_array_values(chars);
                    skip_ws_and_comments(chars);
                    if chars.peek() == Some(&'}') {
                        chars.next();
                    }
                    // Determine type by prefix or first value
                    if prefix == "a" {
                        if values.iter().any(|v| v.contains('.')) {
                            let floats: Vec<f64> =
                                values.iter().filter_map(|s| s.parse().ok()).collect();
                            properties.push(FbxProp::ArrayF64(floats));
                        } else {
                            let ints: Vec<i32> =
                                values.iter().filter_map(|s| s.parse().ok()).collect();
                            properties.push(FbxProp::ArrayI32(ints));
                        }
                    }
                }
            }
            None => break,
            _ => {
                chars.next(); // Skip unknown
            }
        }
    }

    // Read children
    let mut children = Vec::new();
    loop {
        skip_ws_and_comments(chars);
        match chars.peek() {
            Some(&'}') => {
                chars.next();
                break;
            }
            None => break,
            _ => {
                if let Some(child) = parse_ascii_node(chars)? {
                    children.push(child);
                } else {
                    // End marker
                    if chars.peek() == Some(&'}') {
                        chars.next();
                    }
                    break;
                }
            }
        }
    }

    Ok(Some(FbxNode {
        name,
        properties,
        children,
    }))
}

fn skip_ws_no_newline(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(&c) = chars.peek() {
        if c == ' ' || c == '\t' {
            chars.next();
        } else {
            break;
        }
    }
}

fn parse_ascii_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<String> {
    chars.next(); // consume opening "
    let mut s = String::new();
    loop {
        match chars.next() {
            Some('"') => return Ok(s),
            Some('\\') => {
                if let Some(c) = chars.next() {
                    s.push(c);
                }
            }
            Some(c) => s.push(c),
            None => return Err(DxfError::ImportError("Unterminated FBX string".to_string())),
        }
    }
}

fn parse_ascii_number_str(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E' {
            s.push(c);
            chars.next();
        } else {
            break;
        }
    }
    s
}

fn parse_ascii_array_values(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();

    loop {
        skip_ws_and_comments(chars);
        match chars.peek() {
            Some(&'}') | None => {
                if !current.is_empty() {
                    values.push(current);
                }
                break;
            }
            Some(&',') => {
                chars.next();
                if !current.is_empty() {
                    values.push(current.clone());
                    current.clear();
                }
            }
            Some(&c) if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' => {
                current.push(c);
                chars.next();
                // Continue reading the number
                while let Some(&nc) = chars.peek() {
                    if nc.is_ascii_digit()
                        || nc == '.'
                        || nc == '-'
                        || nc == '+'
                        || nc == 'e'
                        || nc == 'E'
                    {
                        current.push(nc);
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            _ => {
                chars.next();
            }
        }
    }

    values
}

// ─── Geometry extraction ─────────────────────────────────────────────────

fn extract_geometry(node: &FbxNode) -> Option<FbxGeometry> {
    let id = node.prop_i64(0)?;
    let raw_name = node.prop_str(1).unwrap_or("geometry");
    let name = raw_name
        .split("::")
        .last()
        .unwrap_or(raw_name)
        .trim()
        .to_string();

    let subtype = node.prop_str(2).unwrap_or("");
    if subtype != "Mesh" && !subtype.is_empty() {
        return None; // Not a mesh geometry
    }

    // Get vertices (Vertices node with f64 array property)
    let vertices_arr = node.child("Vertices").and_then(|n| n.prop_f64_array(0))?;
    if vertices_arr.len() < 3 {
        return None;
    }

    let vertices: Vec<[f64; 3]> = vertices_arr
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();

    // Get polygon indices
    let indices = node
        .child("PolygonVertexIndex")
        .and_then(|n| n.prop_i32_array(0))?
        .clone();

    Some(FbxGeometry {
        id,
        name,
        vertices,
        polygon_indices: indices,
    })
}

fn extract_material(node: &FbxNode) -> Option<FbxMaterialInfo> {
    let id = node.prop_i64(0)?;
    let raw_name = node.prop_str(1).unwrap_or("material");
    let name = raw_name
        .split("::")
        .last()
        .unwrap_or(raw_name)
        .trim()
        .to_string();

    let mut diffuse = [0.8f32, 0.8, 0.8];

    // Look for Properties70 → P with "DiffuseColor"
    if let Some(props70) = node.child("Properties70") {
        for p in props70.children_named("P") {
            if p.prop_str(0) == Some("DiffuseColor") {
                // DiffuseColor property: P: "DiffuseColor", "Color", "", "A", r, g, b
                if p.properties.len() >= 7 {
                    let r = match &p.properties[4] {
                        FbxProp::F64(v) => *v as f32,
                        FbxProp::F32(v) => *v,
                        _ => continue,
                    };
                    let g = match &p.properties[5] {
                        FbxProp::F64(v) => *v as f32,
                        FbxProp::F32(v) => *v,
                        _ => continue,
                    };
                    let b = match &p.properties[6] {
                        FbxProp::F64(v) => *v as f32,
                        FbxProp::F32(v) => *v,
                        _ => continue,
                    };
                    diffuse = [r, g, b];
                }
            }
        }
    }

    Some(FbxMaterialInfo { id, name, diffuse })
}

// ─── Mesh building ──────────────────────────────────────────────────────

/// Build a Mesh from FBX polygon vertex indices.
///
/// FBX encodes polygons as sequences of vertex indices where the last index
/// of each polygon is negated and decremented by 1 (bitwise NOT).
fn build_fbx_mesh(
    vertices: &[[f64; 3]],
    polygon_indices: &[i32],
    scale: f64,
    merge: bool,
    tolerance: f64,
) -> Mesh {
    // First, decode polygons and triangulate
    let mut triangles: Vec<[usize; 3]> = Vec::new();
    let mut poly: Vec<usize> = Vec::new();

    for &idx in polygon_indices {
        if idx < 0 {
            // Last vertex of polygon: decode as -(idx + 1) = !idx
            let actual = (!idx) as usize;
            poly.push(actual);

            // Triangulate via fan
            if poly.len() >= 3 {
                for i in 1..poly.len() - 1 {
                    triangles.push([poly[0], poly[i], poly[i + 1]]);
                }
            }
            poly.clear();
        } else {
            poly.push(idx as usize);
        }
    }

    if merge {
        build_merged_fbx(vertices, &triangles, scale, tolerance)
    } else {
        build_unmerged_fbx(vertices, &triangles, scale)
    }
}

fn build_unmerged_fbx(positions: &[[f64; 3]], triangles: &[[usize; 3]], scale: f64) -> Mesh {
    let mut verts = Vec::with_capacity(triangles.len() * 3);
    let mut faces = Vec::with_capacity(triangles.len());

    for tri in triangles {
        let base = verts.len();
        for &idx in tri {
            let p = if idx < positions.len() {
                positions[idx]
            } else {
                [0.0; 3]
            };
            verts.push(Vector3::new(p[0] * scale, p[1] * scale, p[2] * scale));
        }
        faces.push(MeshFace::triangle(base, base + 1, base + 2));
    }

    let mut mesh = Mesh::new();
    mesh.vertices = verts;
    mesh.faces = faces;
    mesh.compute_edges();
    mesh
}

fn build_merged_fbx(
    positions: &[[f64; 3]],
    triangles: &[[usize; 3]],
    scale: f64,
    tolerance: f64,
) -> Mesh {
    let inv_tol = if tolerance > 0.0 {
        1.0 / tolerance
    } else {
        1e9
    };
    let mut mesh_verts: Vec<Vector3> = Vec::new();
    let mut vert_map: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut faces = Vec::with_capacity(triangles.len());

    let mut get_or_insert = |p: [f64; 3]| -> usize {
        let x = p[0] * scale;
        let y = p[1] * scale;
        let z = p[2] * scale;
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

    for tri in triangles {
        let p0 = if tri[0] < positions.len() {
            positions[tri[0]]
        } else {
            [0.0; 3]
        };
        let p1 = if tri[1] < positions.len() {
            positions[tri[1]]
        } else {
            [0.0; 3]
        };
        let p2 = if tri[2] < positions.len() {
            positions[tri[2]]
        } else {
            [0.0; 3]
        };
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fbx_polygon_decode() {
        // Triangle: [0, 1, -3] → vertices 0, 1, 2 (bitwise NOT of -3 = 2)
        let indices = vec![0, 1, -3];
        let verts = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let mesh = build_fbx_mesh(&verts, &indices, 1.0, false, 1e-9);
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.faces.len(), 1);
    }

    #[test]
    fn test_fbx_quad_decode() {
        // Quad: [0, 1, 2, -4] → 0, 1, 2, 3 → 2 triangles
        let indices = vec![0, 1, 2, -4];
        let verts = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let mesh = build_fbx_mesh(&verts, &indices, 1.0, false, 1e-9);
        assert_eq!(mesh.faces.len(), 2); // Quad → 2 triangles
    }

    #[test]
    fn test_fbx_multi_poly() {
        // Two triangles: [0, 1, -3, 0, 2, -4]
        let indices = vec![0, 1, -3, 0, 2, -4];
        let verts = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let mesh = build_fbx_mesh(&verts, &indices, 1.0, false, 1e-9);
        assert_eq!(mesh.faces.len(), 2);
    }

    #[test]
    fn test_is_binary_fbx() {
        assert!(!is_binary_fbx(b"not fbx"));
        let mut magic = FBX_MAGIC.to_vec();
        magic.extend_from_slice(&[0u8; 6]); // pad to 27
        assert!(is_binary_fbx(&magic));
    }

    #[test]
    fn test_extract_geometry() {
        let node = FbxNode {
            name: "Geometry".to_string(),
            properties: vec![
                FbxProp::I64(12345),
                FbxProp::Str("Mesh::MyMesh".to_string()),
                FbxProp::Str("Mesh".to_string()),
            ],
            children: vec![
                FbxNode {
                    name: "Vertices".to_string(),
                    properties: vec![FbxProp::ArrayF64(vec![
                        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                    ])],
                    children: Vec::new(),
                },
                FbxNode {
                    name: "PolygonVertexIndex".to_string(),
                    properties: vec![FbxProp::ArrayI32(vec![0, 1, -3])],
                    children: Vec::new(),
                },
            ],
        };

        let geom = extract_geometry(&node).unwrap();
        assert_eq!(geom.id, 12345);
        assert_eq!(geom.name, "MyMesh");
        assert_eq!(geom.vertices.len(), 3);
        assert_eq!(geom.polygon_indices, vec![0, 1, -3]);
    }

    #[test]
    fn test_ascii_fbx_simple() {
        let fbx_text = br#"Objects: {
    Geometry: 100, "Geometry::Mesh", "Mesh" {
        Vertices: *9 {
            a: 0.0,0.0,0.0,1.0,0.0,0.0,0.0,1.0,0.0
        }
        PolygonVertexIndex: *3 {
            a: 0,1,-3
        }
    }
}
"#;
        let nodes = parse_ascii_fbx(fbx_text).unwrap();
        let objects = nodes.iter().find(|n| n.name == "Objects").unwrap();
        let geom_nodes = objects.children_named("Geometry");
        assert_eq!(geom_nodes.len(), 1);
    }
}
