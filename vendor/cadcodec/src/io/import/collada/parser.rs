//! COLLADA (.dae) XML parser.
//!
//! Extracts mesh geometries, materials with diffuse colours, and visual-scene
//! transform hierarchies from a COLLADA 1.4 / 1.5 document.

use std::collections::HashMap;
use std::io::BufRead;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{DxfError, Result};

/// A parsed COLLADA scene.
#[derive(Debug, Clone)]
pub struct ColladaScene {
    /// Geometries keyed by their `id` attribute.
    pub geometries: Vec<ColladaGeometry>,
    /// Materials keyed by their `id` attribute.
    pub materials: HashMap<String, ColladaMaterial>,
    /// Visual-scene nodes (geometry instances with transforms).
    pub nodes: Vec<ColladaNode>,
}

/// A single mesh geometry extracted from `<library_geometries>`.
#[derive(Debug, Clone)]
pub struct ColladaGeometry {
    pub id: String,
    pub name: String,
    pub vertices: Vec<[f64; 3]>,
    /// Each entry is a vector of vertex indices forming a triangle.
    pub triangles: Vec<ColladaPrimitive>,
}

/// A primitive group (triangles or polygons converted to triangles).
#[derive(Debug, Clone)]
pub struct ColladaPrimitive {
    /// Indices into the geometry's vertex array (groups of 3 for triangles).
    pub indices: Vec<usize>,
    /// Number of vertex attributes per index (stride). We only use the
    /// VERTEX/POSITION input, but skip the others.
    pub input_count: usize,
    /// Offset of the VERTEX/POSITION input within each index group.
    pub vertex_offset: usize,
    /// Material symbol binding (references `<bind_material>` in the instance).
    pub material_symbol: String,
}

/// Material with diffuse colour.
#[derive(Debug, Clone)]
pub struct ColladaMaterial {
    pub id: String,
    pub name: String,
    /// Diffuse colour (R, G, B, A) in 0.0–1.0 range.
    pub diffuse: [f32; 4],
}

/// A visual-scene node referencing a geometry with a transform.
#[derive(Debug, Clone)]
pub struct ColladaNode {
    pub name: String,
    /// Geometry URL (e.g. `#geom0`).
    pub geometry_url: String,
    /// Flattened 4×4 column-major transform matrix (identity if none).
    pub transform: [f64; 16],
    /// Material bindings: symbol → material id.
    pub material_bindings: HashMap<String, String>,
}

/// Parse a COLLADA XML document from a buffered reader.
pub fn parse_collada<R: BufRead>(reader: R) -> Result<ColladaScene> {
    let mut xml = Reader::from_reader(reader);
    xml.config_mut().trim_text(true);

    let mut scene = ColladaScene {
        geometries: Vec::new(),
        materials: HashMap::new(),
        nodes: Vec::new(),
    };

    // Effect id → diffuse colour (effects are referenced by materials)
    let mut effects: HashMap<String, [f32; 4]> = HashMap::new();
    // Material id → effect URL
    let mut material_to_effect: HashMap<String, String> = HashMap::new();

    let mut buf = Vec::new();

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"geometry" => {
                        let geom = parse_geometry(&mut xml, &e)?;
                        scene.geometries.push(geom);
                    }
                    b"effect" => {
                        let id = attr_string(&e, b"id");
                        let color = parse_effect(&mut xml)?;
                        if let Some(id) = id {
                            effects.insert(id, color);
                        }
                    }
                    b"material" => {
                        let id = attr_string(&e, b"id");
                        let name = attr_string(&e, b"name")
                            .or_else(|| id.clone())
                            .unwrap_or_default();
                        // Read inner <instance_effect url="#effect_id"/>
                        let effect_url = parse_material_element(&mut xml)?;
                        if let (Some(id), Some(url)) = (id, effect_url) {
                            material_to_effect.insert(id.clone(), url);
                            // Placeholder – colour resolved later
                            scene.materials.insert(
                                id.clone(),
                                ColladaMaterial {
                                    id,
                                    name,
                                    diffuse: [0.8, 0.8, 0.8, 1.0],
                                },
                            );
                        }
                    }
                    b"visual_scene" => {
                        parse_visual_scene(&mut xml, &mut scene.nodes)?;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!(
                    "COLLADA XML parse error: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    // Resolve material → effect → diffuse colour
    for (mat_id, mat) in &mut scene.materials {
        if let Some(effect_url) = material_to_effect.get(mat_id) {
            let effect_id = effect_url.strip_prefix('#').unwrap_or(effect_url);
            if let Some(color) = effects.get(effect_id) {
                mat.diffuse = *color;
            }
        }
    }

    // If no visual_scene nodes exist, create one node per geometry
    if scene.nodes.is_empty() {
        for geom in &scene.geometries {
            scene.nodes.push(ColladaNode {
                name: geom.name.clone(),
                geometry_url: format!("#{}", geom.id),
                transform: IDENTITY_MATRIX,
                material_bindings: HashMap::new(),
            });
        }
    }

    Ok(scene)
}

const IDENTITY_MATRIX: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

// ─── Geometry parsing ────────────────────────────────────────────────────

fn parse_geometry<R: BufRead>(
    xml: &mut Reader<R>,
    start: &quick_xml::events::BytesStart<'_>,
) -> Result<ColladaGeometry> {
    let id = attr_string(start, b"id").unwrap_or_default();
    let name = attr_string(start, b"name").unwrap_or_else(|| id.clone());

    let mut geom = ColladaGeometry {
        id,
        name,
        vertices: Vec::new(),
        triangles: Vec::new(),
    };

    // Source arrays keyed by id (e.g. "mesh-positions-array")
    let mut sources: HashMap<String, Vec<f64>> = HashMap::new();
    // <vertices> element maps its id to a source id
    let mut vertices_source: Option<(String, String)> = None;

    let mut buf = Vec::new();
    let mut depth = 1u32;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"source" => {
                        let src_id = attr_string(e, b"id").unwrap_or_default();
                        let floats = parse_source_floats(xml, &mut depth)?;
                        sources.insert(src_id, floats);
                    }
                    b"vertices" => {
                        let vert_id = attr_string(e, b"id").unwrap_or_default();
                        let source_ref = parse_vertices_element(xml, &mut depth)?;
                        vertices_source = Some((vert_id, source_ref));
                    }
                    b"triangles" | b"polylist" | b"polygons" => {
                        let prim = parse_primitives(xml, local, e, &mut depth)?;
                        geom.triangles.push(prim);
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref _e)) => {
                // Handle self-closing elements like <input ... />
                // These are consumed within their parent parsers
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!(
                    "Error parsing geometry: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    // Resolve vertex positions
    if let Some((_vert_id, source_ref)) = &vertices_source {
        let source_id = source_ref.strip_prefix('#').unwrap_or(source_ref);
        if let Some(floats) = sources.get(source_id) {
            geom.vertices = floats.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        }
    } else {
        // Fallback: look for a source with "position" in the name
        for (sid, floats) in &sources {
            if sid.to_lowercase().contains("position") {
                geom.vertices = floats.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
                break;
            }
        }
    }

    Ok(geom)
}

fn parse_source_floats<R: BufRead>(xml: &mut Reader<R>, depth: &mut u32) -> Result<Vec<f64>> {
    let mut floats = Vec::new();
    let mut buf = Vec::new();
    let entry_depth = *depth;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                *depth += 1;
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"float_array" {
                    // Read text content
                    let text = read_text_content(xml, depth)?;
                    floats = parse_float_list(&text);
                }
            }
            Ok(Event::End(_)) => {
                *depth -= 1;
                if *depth < entry_depth {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!(
                    "Error parsing source: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(floats)
}

fn parse_vertices_element<R: BufRead>(xml: &mut Reader<R>, depth: &mut u32) -> Result<String> {
    let mut source_ref = String::new();
    let mut buf = Vec::new();
    let entry_depth = *depth;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"input" {
                    let semantic = attr_string(e, b"semantic").unwrap_or_default();
                    if semantic == "POSITION" {
                        source_ref = attr_string(e, b"source").unwrap_or_default();
                    }
                }
            }
            Ok(Event::Start(ref e)) => {
                *depth += 1;
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"input" {
                    let semantic = attr_string(e, b"semantic").unwrap_or_default();
                    if semantic == "POSITION" {
                        source_ref = attr_string(e, b"source").unwrap_or_default();
                    }
                }
            }
            Ok(Event::End(_)) => {
                *depth -= 1;
                if *depth < entry_depth {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!(
                    "Error parsing vertices: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(source_ref)
}

fn parse_primitives<R: BufRead>(
    xml: &mut Reader<R>,
    prim_type: &[u8],
    start: &quick_xml::events::BytesStart<'_>,
    depth: &mut u32,
) -> Result<ColladaPrimitive> {
    let material_symbol = attr_string(start, b"material").unwrap_or_default();
    let _count: usize = attr_string(start, b"count")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let mut inputs: Vec<(String, usize)> = Vec::new(); // (semantic, offset)
    let mut indices_text = String::new();
    let mut vcount_text = String::new();
    let mut buf = Vec::new();
    let entry_depth = *depth;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"input" {
                    let semantic = attr_string(e, b"semantic").unwrap_or_default();
                    let offset: usize = attr_string(e, b"offset")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    inputs.push((semantic, offset));
                }
            }
            Ok(Event::Start(ref e)) => {
                *depth += 1;
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"input" => {
                        let semantic = attr_string(e, b"semantic").unwrap_or_default();
                        let offset: usize = attr_string(e, b"offset")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        inputs.push((semantic, offset));
                    }
                    b"p" => {
                        indices_text = read_text_content(xml, depth)?;
                    }
                    b"vcount" => {
                        vcount_text = read_text_content(xml, depth)?;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(_)) => {
                *depth -= 1;
                if *depth < entry_depth {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!(
                    "Error parsing primitives: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    let input_count = if inputs.is_empty() {
        1
    } else {
        inputs.iter().map(|(_, o)| o + 1).max().unwrap_or(1)
    };

    let vertex_offset = inputs
        .iter()
        .find(|(s, _)| s == "VERTEX" || s == "POSITION")
        .map(|(_, o)| *o)
        .unwrap_or(0);

    let all_indices: Vec<usize> = indices_text
        .split_whitespace()
        .filter_map(|s| s.parse::<usize>().ok())
        .collect();

    // For polylist/polygons, convert to triangles via fan triangulation
    let triangle_indices = if prim_type == b"polylist" && !vcount_text.is_empty() {
        let vcounts: Vec<usize> = vcount_text
            .split_whitespace()
            .filter_map(|s| s.parse::<usize>().ok())
            .collect();
        polylist_to_triangles(&all_indices, &vcounts, input_count, vertex_offset)
    } else if prim_type == b"triangles" {
        // Already triangles — just extract vertex indices
        extract_vertex_indices(&all_indices, input_count, vertex_offset)
    } else {
        // polygons — treat each <p> as a polygon, but we only have one <p> here
        // Treat as triangles
        extract_vertex_indices(&all_indices, input_count, vertex_offset)
    };

    Ok(ColladaPrimitive {
        indices: triangle_indices,
        input_count,
        vertex_offset,
        material_symbol,
    })
}

/// Extract just the vertex position indices from interleaved index data.
fn extract_vertex_indices(
    all_indices: &[usize],
    input_count: usize,
    vertex_offset: usize,
) -> Vec<usize> {
    all_indices
        .chunks(input_count)
        .map(|chunk| {
            if vertex_offset < chunk.len() {
                chunk[vertex_offset]
            } else {
                0
            }
        })
        .collect()
}

/// Convert polylist variable-count faces to triangles using fan triangulation.
fn polylist_to_triangles(
    all_indices: &[usize],
    vcounts: &[usize],
    input_count: usize,
    vertex_offset: usize,
) -> Vec<usize> {
    let mut result = Vec::new();
    let mut pos = 0;

    for &vc in vcounts {
        let face_start = pos;
        let face_verts: Vec<usize> = (0..vc)
            .map(|i| {
                let idx = face_start + i * input_count + vertex_offset;
                if idx < all_indices.len() {
                    all_indices[idx]
                } else {
                    0
                }
            })
            .collect();

        // Fan triangulation: (v0, v1, v2), (v0, v2, v3), ...
        if face_verts.len() >= 3 {
            for i in 1..face_verts.len() - 1 {
                result.push(face_verts[0]);
                result.push(face_verts[i]);
                result.push(face_verts[i + 1]);
            }
        }

        pos += vc * input_count;
    }

    result
}

// ─── Effect parsing ──────────────────────────────────────────────────────

fn parse_effect<R: BufRead>(xml: &mut Reader<R>) -> Result<[f32; 4]> {
    let mut diffuse = [0.8f32, 0.8, 0.8, 1.0];
    let mut buf = Vec::new();
    let mut depth = 1u32;
    let mut in_diffuse = false;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"diffuse" {
                    in_diffuse = true;
                }
                if in_diffuse && local == b"color" {
                    let text = read_text_content(xml, &mut depth)?;
                    let vals: Vec<f32> = text
                        .split_whitespace()
                        .filter_map(|s| s.parse::<f32>().ok())
                        .collect();
                    if vals.len() >= 3 {
                        diffuse[0] = vals[0];
                        diffuse[1] = vals[1];
                        diffuse[2] = vals[2];
                        if vals.len() >= 4 {
                            diffuse[3] = vals[3];
                        }
                    }
                    in_diffuse = false;
                }
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                // Reset in_diffuse when leaving <diffuse>
                if in_diffuse {
                    in_diffuse = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!(
                    "Error parsing effect: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(diffuse)
}

// ─── Material element parsing ────────────────────────────────────────────

fn parse_material_element<R: BufRead>(xml: &mut Reader<R>) -> Result<Option<String>> {
    let mut effect_url = None;
    let mut buf = Vec::new();
    let mut depth = 1u32;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"instance_effect" {
                    effect_url = attr_string(e, b"url");
                }
            }
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"instance_effect" {
                    effect_url = attr_string(e, b"url");
                }
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!(
                    "Error parsing material: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(effect_url)
}

// ─── Visual scene parsing ────────────────────────────────────────────────

fn parse_visual_scene<R: BufRead>(xml: &mut Reader<R>, nodes: &mut Vec<ColladaNode>) -> Result<()> {
    let mut buf = Vec::new();
    let mut depth = 1u32;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"node" {
                    let name = attr_string(e, b"name").unwrap_or_default();
                    let node = parse_node(xml, &name, &mut depth)?;
                    if let Some(n) = node {
                        nodes.push(n);
                    }
                }
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!(
                    "Error parsing visual_scene: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}

fn parse_node<R: BufRead>(
    xml: &mut Reader<R>,
    name: &str,
    depth: &mut u32,
) -> Result<Option<ColladaNode>> {
    let mut transform = IDENTITY_MATRIX;
    let mut geometry_url = String::new();
    let mut material_bindings: HashMap<String, String> = HashMap::new();
    let mut buf = Vec::new();

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                *depth += 1;
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"matrix" => {
                        let text = read_text_content(xml, depth)?;
                        let vals: Vec<f64> = text
                            .split_whitespace()
                            .filter_map(|s| s.parse::<f64>().ok())
                            .collect();
                        if vals.len() == 16 {
                            // COLLADA matrices are row-major; convert to column-major
                            transform = row_to_col_major(&vals);
                        }
                    }
                    b"instance_geometry" => {
                        geometry_url = attr_string(e, b"url").unwrap_or_default();
                        // Parse bind_material inside
                        parse_instance_geometry_bindings(xml, depth, &mut material_bindings)?;
                    }
                    b"node" => {
                        // Skip nested nodes for now
                        skip_element(xml, depth)?;
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"instance_geometry" {
                    geometry_url = attr_string(e, b"url").unwrap_or_default();
                }
            }
            Ok(Event::End(_)) => {
                *depth -= 1;
                break;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!("Error parsing node: {}", e)));
            }
            _ => {}
        }
        buf.clear();
    }

    if geometry_url.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ColladaNode {
            name: name.to_string(),
            geometry_url,
            transform,
            material_bindings,
        }))
    }
}

fn parse_instance_geometry_bindings<R: BufRead>(
    xml: &mut Reader<R>,
    depth: &mut u32,
    bindings: &mut HashMap<String, String>,
) -> Result<()> {
    let mut buf = Vec::new();

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"instance_material" {
                    let symbol = attr_string(e, b"symbol").unwrap_or_default();
                    let target = attr_string(e, b"target").unwrap_or_default();
                    let target = target.strip_prefix('#').unwrap_or(&target).to_string();
                    if !symbol.is_empty() {
                        bindings.insert(symbol, target);
                    }
                }
            }
            Ok(Event::Start(ref e)) => {
                *depth += 1;
                let name = e.name();
                let local = local_name(name.as_ref());
                if local == b"instance_material" {
                    let symbol = attr_string(e, b"symbol").unwrap_or_default();
                    let target = attr_string(e, b"target").unwrap_or_default();
                    let target = target.strip_prefix('#').unwrap_or(&target).to_string();
                    if !symbol.is_empty() {
                        bindings.insert(symbol, target);
                    }
                }
            }
            Ok(Event::End(_)) => {
                *depth -= 1;
                break;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!(
                    "Error parsing instance bindings: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────

fn read_text_content<R: BufRead>(xml: &mut Reader<R>, depth: &mut u32) -> Result<String> {
    let mut text = String::new();
    let mut buf = Vec::new();

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Text(ref e)) => {
                text.push_str(
                    &e.unescape().map_err(|er| {
                        DxfError::ImportError(format!("XML unescape error: {}", er))
                    })?,
                );
            }
            Ok(Event::End(_)) => {
                *depth -= 1;
                break;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!("XML read error: {}", e)));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(text)
}

fn skip_element<R: BufRead>(xml: &mut Reader<R>, depth: &mut u32) -> Result<()> {
    let mut buf = Vec::new();
    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(_)) => {
                *depth += 1;
            }
            Ok(Event::End(_)) => {
                *depth -= 1;
                break;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DxfError::ImportError(format!("XML skip error: {}", e)));
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

fn parse_float_list(text: &str) -> Vec<f64> {
    text.split_whitespace()
        .filter_map(|s| s.parse::<f64>().ok())
        .collect()
}

/// Get the local name (without namespace prefix) of an element.
fn local_name(full: &[u8]) -> &[u8] {
    if let Some(pos) = full.iter().position(|&b| b == b':') {
        &full[pos + 1..]
    } else {
        full
    }
}

/// Read a UTF-8 attribute value from a start element.
fn attr_string(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == key {
            return String::from_utf8(attr.value.to_vec()).ok();
        }
    }
    None
}

/// Convert a row-major 4×4 matrix to column-major.
fn row_to_col_major(row: &[f64]) -> [f64; 16] {
    [
        row[0], row[4], row[8], row[12], row[1], row[5], row[9], row[13], row[2], row[6], row[10],
        row[14], row[3], row[7], row[11], row[15],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    const SAMPLE_DAE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <library_effects>
    <effect id="effect0">
      <profile_COMMON>
        <technique sid="common">
          <phong>
            <diffuse>
              <color>1.0 0.0 0.0 1.0</color>
            </diffuse>
          </phong>
        </technique>
      </profile_COMMON>
    </effect>
  </library_effects>
  <library_materials>
    <material id="material0" name="RedMat">
      <instance_effect url="#effect0"/>
    </material>
  </library_materials>
  <library_geometries>
    <geometry id="geom0" name="Triangle">
      <mesh>
        <source id="positions">
          <float_array id="positions-array" count="9">
            0 0 0  1 0 0  0 1 0
          </float_array>
          <technique_common>
            <accessor source="#positions-array" count="3" stride="3">
              <param name="X" type="float"/>
              <param name="Y" type="float"/>
              <param name="Z" type="float"/>
            </accessor>
          </technique_common>
        </source>
        <vertices id="verts">
          <input semantic="POSITION" source="#positions"/>
        </vertices>
        <triangles count="1" material="mat_sym">
          <input semantic="VERTEX" source="#verts" offset="0"/>
          <p>0 1 2</p>
        </triangles>
      </mesh>
    </geometry>
  </library_geometries>
  <library_visual_scenes>
    <visual_scene id="Scene" name="Scene">
      <node name="TriNode">
        <instance_geometry url="#geom0">
          <bind_material>
            <technique_common>
              <instance_material symbol="mat_sym" target="#material0"/>
            </technique_common>
          </bind_material>
        </instance_geometry>
      </node>
    </visual_scene>
  </library_visual_scenes>
</COLLADA>"##;

    #[test]
    fn test_parse_sample_dae() {
        let reader = BufReader::new(SAMPLE_DAE.as_bytes());
        let scene = parse_collada(reader).unwrap();

        assert_eq!(scene.geometries.len(), 1);
        assert_eq!(scene.geometries[0].name, "Triangle");
        assert_eq!(scene.geometries[0].vertices.len(), 3);
        assert_eq!(scene.geometries[0].triangles.len(), 1);
        assert_eq!(scene.geometries[0].triangles[0].indices, vec![0, 1, 2]);

        assert_eq!(scene.materials.len(), 1);
        let mat = scene.materials.get("material0").unwrap();
        assert_eq!(mat.name, "RedMat");
        assert!((mat.diffuse[0] - 1.0).abs() < 0.01);
        assert!((mat.diffuse[1] - 0.0).abs() < 0.01);

        assert_eq!(scene.nodes.len(), 1);
        assert_eq!(scene.nodes[0].name, "TriNode");
        assert_eq!(scene.nodes[0].geometry_url, "#geom0");
        assert_eq!(
            scene.nodes[0].material_bindings.get("mat_sym"),
            Some(&"material0".to_string())
        );
    }

    #[test]
    fn test_row_to_col_major() {
        let row = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0,
        ];
        let col = row_to_col_major(&row);
        // Column 0: row[0], row[4], row[8], row[12]
        assert_eq!(col[0], 1.0);
        assert_eq!(col[1], 5.0);
        assert_eq!(col[2], 9.0);
        assert_eq!(col[3], 13.0);
    }
}
