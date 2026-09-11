//! COLLADA (.dae) importer.
//!
//! Produces one [`Mesh`] entity per geometry instance, with a DXF layer per
//! unique material and diffuse-colour mapping.
//!
//! # Example
//!
//! ```rust,ignore
//! use acadrust::io::import::collada::ColladaImporter;
//! use acadrust::io::import::ImportConfig;
//!
//! let doc = ColladaImporter::from_file("scene.dae")?
//!     .with_config(ImportConfig::default())
//!     .import()?;
//! ```

pub mod parser;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::document::CadDocument;
use crate::entities::mesh::{Mesh, MeshFace};
use crate::entities::EntityType;
use crate::error::{DxfError, Result};
use crate::types::Vector3;

use super::color_mapping::{create_material_layer, rgb_to_color};
use super::ImportConfig;

use parser::{parse_collada, ColladaGeometry, ColladaNode, ColladaScene};

/// COLLADA file importer.
pub struct ColladaImporter {
    data: Vec<u8>,
    config: ImportConfig,
}

impl ColladaImporter {
    /// Create an importer from a file path.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut file = File::open(path).map_err(|e| {
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
            .map_err(|e| DxfError::ImportError(format!("Cannot read COLLADA data: {}", e)))?;
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
        let scene = self.parse()?;
        self.build_document(&scene)
    }

    /// Parse the COLLADA XML into a [`ColladaScene`].
    pub fn parse(&self) -> Result<ColladaScene> {
        let reader = BufReader::new(self.data.as_slice());
        parse_collada(reader)
    }

    /// Convert a [`ColladaScene`] into a [`CadDocument`].
    fn build_document(&self, scene: &ColladaScene) -> Result<CadDocument> {
        let mut doc = CadDocument::new();
        let scale = self.config.scale_factor;

        // Index geometries by id for lookup
        let geom_map: HashMap<&str, &ColladaGeometry> = scene
            .geometries
            .iter()
            .map(|g| (g.id.as_str(), g))
            .collect();

        for node in &scene.nodes {
            let geom_id = node
                .geometry_url
                .strip_prefix('#')
                .unwrap_or(&node.geometry_url);
            let geom = match geom_map.get(geom_id) {
                Some(g) => g,
                None => continue, // Referenced geometry not found — skip
            };

            // Build meshes grouped by material
            let meshes_by_material = build_meshes_for_geometry(
                geom,
                node,
                &scene.materials,
                scale,
                self.config.merge_vertices,
                self.config.merge_tolerance,
            );

            for (material_name, color, mut mesh) in meshes_by_material {
                let layer_name = if material_name.is_empty() {
                    create_material_layer(
                        &mut doc,
                        &self.config.layer_prefix,
                        if node.name.is_empty() {
                            &geom.name
                        } else {
                            &node.name
                        },
                        self.config.default_color,
                    )
                } else {
                    let c = rgb_to_color(
                        (color[0] * 255.0) as u8,
                        (color[1] * 255.0) as u8,
                        (color[2] * 255.0) as u8,
                        true,
                    );
                    create_material_layer(&mut doc, &self.config.layer_prefix, &material_name, c)
                };

                mesh.common.layer = layer_name;
                if !material_name.is_empty() {
                    mesh.common.color = rgb_to_color(
                        (color[0] * 255.0) as u8,
                        (color[1] * 255.0) as u8,
                        (color[2] * 255.0) as u8,
                        true,
                    );
                }

                doc.add_entity(EntityType::Mesh(mesh))?;
            }
        }

        Ok(doc)
    }
}

/// Build Mesh entities from a geometry, grouped by material.
///
/// Returns a list of `(material_name, diffuse_color, Mesh)`.
fn build_meshes_for_geometry(
    geom: &ColladaGeometry,
    node: &ColladaNode,
    materials: &HashMap<String, parser::ColladaMaterial>,
    scale: f64,
    merge: bool,
    tolerance: f64,
) -> Vec<(String, [f32; 4], Mesh)> {
    let has_transform = !is_identity(&node.transform);

    // Group primitives by resolved material id
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new(); // mat_id → triangle vertex indices

    for prim in &geom.triangles {
        // Resolve material: symbol → binding → material id
        let mat_id = node
            .material_bindings
            .get(&prim.material_symbol)
            .cloned()
            .unwrap_or_default();

        let entry = groups.entry(mat_id).or_default();
        entry.extend_from_slice(&prim.indices);
    }

    let mut results = Vec::new();

    for (mat_id, vertex_indices) in groups {
        let diffuse = materials
            .get(&mat_id)
            .map(|m| m.diffuse)
            .unwrap_or([0.8, 0.8, 0.8, 1.0]);
        let mat_name = materials
            .get(&mat_id)
            .map(|m| m.name.clone())
            .unwrap_or_default();

        let mesh = build_mesh_from_indices(
            &geom.vertices,
            &vertex_indices,
            &node.transform,
            has_transform,
            scale,
            merge,
            tolerance,
        );

        results.push((mat_name, diffuse, mesh));
    }

    results
}

fn build_mesh_from_indices(
    positions: &[[f64; 3]],
    vertex_indices: &[usize],
    transform: &[f64; 16],
    has_transform: bool,
    scale: f64,
    merge: bool,
    tolerance: f64,
) -> Mesh {
    // Transform and scale vertex positions
    let transform_vertex = |idx: usize| -> Vector3 {
        if idx >= positions.len() {
            return Vector3::ZERO;
        }
        let p = positions[idx];
        let (x, y, z) = if has_transform {
            apply_transform(p[0], p[1], p[2], transform)
        } else {
            (p[0], p[1], p[2])
        };
        Vector3::new(x * scale, y * scale, z * scale)
    };

    if merge {
        build_merged(vertex_indices, &transform_vertex, tolerance)
    } else {
        build_unmerged(vertex_indices, &transform_vertex)
    }
}

fn build_unmerged(vertex_indices: &[usize], transform: &dyn Fn(usize) -> Vector3) -> Mesh {
    let num_tris = vertex_indices.len() / 3;
    let mut vertices = Vec::with_capacity(num_tris * 3);
    let mut faces = Vec::with_capacity(num_tris);

    for tri in vertex_indices.chunks_exact(3) {
        let base = vertices.len();
        vertices.push(transform(tri[0]));
        vertices.push(transform(tri[1]));
        vertices.push(transform(tri[2]));
        faces.push(MeshFace::triangle(base, base + 1, base + 2));
    }

    let mut mesh = Mesh::new();
    mesh.vertices = vertices;
    mesh.faces = faces;
    mesh.compute_edges();
    mesh
}

fn build_merged(
    vertex_indices: &[usize],
    transform: &dyn Fn(usize) -> Vector3,
    tolerance: f64,
) -> Mesh {
    let inv_tol = if tolerance > 0.0 {
        1.0 / tolerance
    } else {
        1e9
    };

    let num_tris = vertex_indices.len() / 3;
    let mut mesh_vertices: Vec<Vector3> = Vec::new();
    let mut vert_map: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut faces = Vec::with_capacity(num_tris);

    let mut get_or_insert = |v: Vector3| -> usize {
        let key = (
            (v.x * inv_tol).round() as i64,
            (v.y * inv_tol).round() as i64,
            (v.z * inv_tol).round() as i64,
        );
        if let Some(&idx) = vert_map.get(&key) {
            idx
        } else {
            let idx = mesh_vertices.len();
            mesh_vertices.push(v);
            vert_map.insert(key, idx);
            idx
        }
    };

    for tri in vertex_indices.chunks_exact(3) {
        let i0 = get_or_insert(transform(tri[0]));
        let i1 = get_or_insert(transform(tri[1]));
        let i2 = get_or_insert(transform(tri[2]));
        if i0 != i1 && i1 != i2 && i0 != i2 {
            faces.push(MeshFace::triangle(i0, i1, i2));
        }
    }

    let mut mesh = Mesh::new();
    mesh.vertices = mesh_vertices;
    mesh.faces = faces;
    mesh.compute_edges();
    mesh
}

/// Apply a 4×4 column-major transform to a point.
fn apply_transform(x: f64, y: f64, z: f64, m: &[f64; 16]) -> (f64, f64, f64) {
    let ox = m[0] * x + m[4] * y + m[8] * z + m[12];
    let oy = m[1] * x + m[5] * y + m[9] * z + m[13];
    let oz = m[2] * x + m[6] * y + m[10] * z + m[14];
    (ox, oy, oz)
}

fn is_identity(m: &[f64; 16]) -> bool {
    const IDENTITY: [f64; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    m.iter()
        .zip(IDENTITY.iter())
        .all(|(a, b)| (a - b).abs() < 1e-15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_transform_identity() {
        let id = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let (x, y, z) = apply_transform(1.0, 2.0, 3.0, &id);
        assert!((x - 1.0).abs() < 1e-10);
        assert!((y - 2.0).abs() < 1e-10);
        assert!((z - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_apply_transform_translation() {
        // Translation by (10, 20, 30) in column-major
        let m = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 10.0, 20.0, 30.0, 1.0,
        ];
        let (x, y, z) = apply_transform(1.0, 2.0, 3.0, &m);
        assert!((x - 11.0).abs() < 1e-10);
        assert!((y - 22.0).abs() < 1e-10);
        assert!((z - 33.0).abs() < 1e-10);
    }

    #[test]
    fn test_import_collada_bytes() {
        let dae = r##"<?xml version="1.0" encoding="utf-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <library_effects>
    <effect id="eff0">
      <profile_COMMON><technique sid="t"><phong>
        <diffuse><color>0.0 0.0 1.0 1.0</color></diffuse>
      </phong></technique></profile_COMMON>
    </effect>
  </library_effects>
  <library_materials>
    <material id="mat0" name="Blue">
      <instance_effect url="#eff0"/>
    </material>
  </library_materials>
  <library_geometries>
    <geometry id="g0" name="Tri">
      <mesh>
        <source id="p0">
          <float_array id="p0a" count="9">0 0 0 1 0 0 0 1 0</float_array>
          <technique_common>
            <accessor source="#p0a" count="3" stride="3">
              <param name="X" type="float"/>
              <param name="Y" type="float"/>
              <param name="Z" type="float"/>
            </accessor>
          </technique_common>
        </source>
        <vertices id="v0">
          <input semantic="POSITION" source="#p0"/>
        </vertices>
        <triangles count="1" material="ms">
          <input semantic="VERTEX" source="#v0" offset="0"/>
          <p>0 1 2</p>
        </triangles>
      </mesh>
    </geometry>
  </library_geometries>
  <library_visual_scenes>
    <visual_scene id="Scene">
      <node name="N">
        <instance_geometry url="#g0">
          <bind_material><technique_common>
            <instance_material symbol="ms" target="#mat0"/>
          </technique_common></bind_material>
        </instance_geometry>
      </node>
    </visual_scene>
  </library_visual_scenes>
</COLLADA>"##;

        let importer = ColladaImporter::from_bytes(dae.as_bytes().to_vec());
        let doc = importer.import().unwrap();
        let entities: Vec<_> = doc.entities().collect();
        assert_eq!(entities.len(), 1);
        if let EntityType::Mesh(mesh) = &entities[0] {
            assert_eq!(mesh.vertices.len(), 3);
            assert_eq!(mesh.faces.len(), 1);
            // Should be on a "Blue" layer
            assert!(mesh.common.layer.contains("Blue"));
        } else {
            panic!("Expected Mesh entity");
        }
    }
}
