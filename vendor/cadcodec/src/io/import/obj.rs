//! Wavefront OBJ importer.
//!
//! Parses `.obj` files and optional `.mtl` material libraries.
//! Produces one [`Mesh`] entity per material group.
//!
//! # Example
//!
//! ```rust,ignore
//! use acadrust::io::import::obj::ObjImporter;
//!
//! let doc = ObjImporter::from_file("model.obj")?.import()?;
//! ```

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::document::CadDocument;
use crate::entities::mesh::{Mesh, MeshFace};
use crate::entities::EntityType;
use crate::error::{DxfError, Result};
use crate::types::Vector3;

use super::color_mapping::{create_material_layer, rgb_to_color};
use super::ImportConfig;

/// Parsed OBJ material from an MTL file.
#[derive(Debug, Clone)]
struct ObjMaterial {
    name: String,
    /// Diffuse colour (Kd) — RGB in 0.0–1.0.
    diffuse: [f32; 3],
}

/// A group of faces sharing the same material.
#[derive(Debug)]
struct FaceGroup {
    material: String,
    /// Each face is a list of vertex indices (0-based).
    faces: Vec<Vec<usize>>,
}

/// OBJ file importer.
pub struct ObjImporter {
    data: Vec<u8>,
    /// Directory of the source file (for resolving mtllib paths).
    base_dir: Option<PathBuf>,
    config: ImportConfig,
}

impl ObjImporter {
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

    /// Create an importer from an in-memory buffer (no MTL resolution).
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self {
            data,
            base_dir: None,
            config: ImportConfig::default(),
        }
    }

    /// Create an importer from a reader (no MTL resolution).
    pub fn from_reader<R: Read>(mut reader: R) -> Result<Self> {
        let mut data = Vec::new();
        reader
            .read_to_end(&mut data)
            .map_err(|e| DxfError::ImportError(format!("Cannot read OBJ data: {}", e)))?;
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
        let text = String::from_utf8_lossy(&self.data);
        let (vertices, groups, mtl_files) = parse_obj(&text)?;
        let materials = self.load_materials(&mtl_files);
        self.build_document(&vertices, &groups, &materials)
    }

    fn load_materials(&self, mtl_files: &[String]) -> HashMap<String, ObjMaterial> {
        let mut materials = HashMap::new();
        let base_dir = match &self.base_dir {
            Some(d) => d,
            None => return materials,
        };
        for mtl_file in mtl_files {
            let mtl_path = base_dir.join(mtl_file);
            if let Ok(content) = fs::read_to_string(&mtl_path) {
                parse_mtl(&content, &mut materials);
            }
        }
        materials
    }

    fn build_document(
        &self,
        vertices: &[[f64; 3]],
        groups: &[FaceGroup],
        materials: &HashMap<String, ObjMaterial>,
    ) -> Result<CadDocument> {
        let mut doc = CadDocument::new();
        let scale = self.config.scale_factor;

        for group in groups {
            if group.faces.is_empty() {
                continue;
            }

            let (layer_name, color) = if let Some(mat) = materials.get(&group.material) {
                let c = rgb_to_color(
                    (mat.diffuse[0] * 255.0) as u8,
                    (mat.diffuse[1] * 255.0) as u8,
                    (mat.diffuse[2] * 255.0) as u8,
                    true,
                );
                (
                    create_material_layer(&mut doc, &self.config.layer_prefix, &mat.name, c),
                    c,
                )
            } else {
                let name = if group.material.is_empty() {
                    "default".to_string()
                } else {
                    group.material.clone()
                };
                (
                    create_material_layer(
                        &mut doc,
                        &self.config.layer_prefix,
                        &name,
                        self.config.default_color,
                    ),
                    self.config.default_color,
                )
            };

            let mesh = build_mesh_from_faces(
                vertices,
                &group.faces,
                scale,
                self.config.merge_vertices,
                self.config.merge_tolerance,
            );
            let mut mesh_entity = mesh;
            mesh_entity.common.layer = layer_name;
            mesh_entity.common.color = color;
            doc.add_entity(EntityType::Mesh(mesh_entity))?;
        }

        Ok(doc)
    }
}

/// Parse OBJ text into vertices, face groups, and mtllib references.
fn parse_obj(text: &str) -> Result<(Vec<[f64; 3]>, Vec<FaceGroup>, Vec<String>)> {
    let mut vertices: Vec<[f64; 3]> = Vec::new();
    let mut mtl_files: Vec<String> = Vec::new();
    let mut groups: Vec<FaceGroup> = Vec::new();
    let mut current_material = String::new();

    // Start with a default group
    groups.push(FaceGroup {
        material: current_material.clone(),
        faces: Vec::new(),
    });

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let keyword = match parts.next() {
            Some(k) => k,
            None => continue,
        };

        match keyword {
            "v" => {
                let coords: Vec<f64> = parts.filter_map(|s| s.parse().ok()).collect();
                if coords.len() >= 3 {
                    vertices.push([coords[0], coords[1], coords[2]]);
                }
            }
            "f" => {
                let face_indices: Vec<usize> = parts
                    .filter_map(|s| {
                        // OBJ face: v, v/vt, v/vt/vn, v//vn — extract first index
                        let idx_str = s.split('/').next()?;
                        let idx: isize = idx_str.parse().ok()?;
                        if idx > 0 {
                            Some((idx - 1) as usize) // 1-based → 0-based
                        } else if idx < 0 {
                            // Negative index = relative to end
                            let abs = (-idx) as usize;
                            if abs <= vertices.len() {
                                Some(vertices.len() - abs)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect();

                if face_indices.len() >= 3 {
                    if let Some(group) = groups.last_mut() {
                        group.faces.push(face_indices);
                    }
                }
            }
            "usemtl" => {
                current_material = parts.collect::<Vec<_>>().join(" ");
                groups.push(FaceGroup {
                    material: current_material.clone(),
                    faces: Vec::new(),
                });
            }
            "mtllib" => {
                let mtl = parts.collect::<Vec<_>>().join(" ");
                if !mtl.is_empty() {
                    mtl_files.push(mtl);
                }
            }
            _ => {} // Ignore vt, vn, g, o, s, etc.
        }
    }

    // Remove empty groups
    groups.retain(|g| !g.faces.is_empty());

    if groups.is_empty() && !vertices.is_empty() {
        // No faces but has vertices — shouldn't happen in well-formed OBJ
        return Err(DxfError::ImportError(
            "OBJ file contains vertices but no faces".to_string(),
        ));
    }

    Ok((vertices, groups, mtl_files))
}

/// Parse an MTL file and add materials to the map.
fn parse_mtl(text: &str, materials: &mut HashMap<String, ObjMaterial>) {
    let mut current: Option<ObjMaterial> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let keyword = match parts.next() {
            Some(k) => k,
            None => continue,
        };

        match keyword {
            "newmtl" => {
                // Save previous material
                if let Some(mat) = current.take() {
                    materials.insert(mat.name.clone(), mat);
                }
                let name = parts.collect::<Vec<_>>().join(" ");
                current = Some(ObjMaterial {
                    name,
                    diffuse: [0.8, 0.8, 0.8],
                });
            }
            "Kd" => {
                if let Some(ref mut mat) = current {
                    let vals: Vec<f32> = parts.filter_map(|s| s.parse().ok()).collect();
                    if vals.len() >= 3 {
                        mat.diffuse = [vals[0], vals[1], vals[2]];
                    }
                }
            }
            _ => {}
        }
    }

    // Save last material
    if let Some(mat) = current {
        materials.insert(mat.name.clone(), mat);
    }
}

/// Build a Mesh from polygon faces, triangulating polygons with > 3 vertices via fan.
fn build_mesh_from_faces(
    vertices: &[[f64; 3]],
    faces: &[Vec<usize>],
    scale: f64,
    merge: bool,
    tolerance: f64,
) -> Mesh {
    // First, triangulate all faces
    let mut triangles: Vec<[usize; 3]> = Vec::new();
    for face in faces {
        if face.len() == 3 {
            triangles.push([face[0], face[1], face[2]]);
        } else if face.len() > 3 {
            // Fan triangulation from first vertex
            for i in 1..face.len() - 1 {
                triangles.push([face[0], face[i], face[i + 1]]);
            }
        }
    }

    if merge {
        build_merged_obj(vertices, &triangles, scale, tolerance)
    } else {
        build_unmerged_obj(vertices, &triangles, scale)
    }
}

fn build_unmerged_obj(positions: &[[f64; 3]], triangles: &[[usize; 3]], scale: f64) -> Mesh {
    let mut verts = Vec::with_capacity(triangles.len() * 3);
    let mut faces = Vec::with_capacity(triangles.len());

    for tri in triangles {
        let base = verts.len();
        for &idx in tri {
            if idx < positions.len() {
                let p = positions[idx];
                verts.push(Vector3::new(p[0] * scale, p[1] * scale, p[2] * scale));
            } else {
                verts.push(Vector3::ZERO);
            }
        }
        faces.push(MeshFace::triangle(base, base + 1, base + 2));
    }

    let mut mesh = Mesh::new();
    mesh.vertices = verts;
    mesh.faces = faces;
    mesh.compute_edges();
    mesh
}

fn build_merged_obj(
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
    fn test_parse_obj_triangle() {
        let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let (verts, groups, _) = parse_obj(obj).unwrap();
        assert_eq!(verts.len(), 3);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].faces.len(), 1);
        assert_eq!(groups[0].faces[0], vec![0, 1, 2]);
    }

    #[test]
    fn test_parse_obj_quad_fan() {
        let obj = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n";
        let (verts, groups, _) = parse_obj(obj).unwrap();
        assert_eq!(verts.len(), 4);
        // Quad should produce one face
        assert_eq!(groups[0].faces.len(), 1);
        assert_eq!(groups[0].faces[0], vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_parse_obj_with_vt_vn() {
        let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1/1/1 2/2/2 3/3/3\n";
        let (_, groups, _) = parse_obj(obj).unwrap();
        assert_eq!(groups[0].faces[0], vec![0, 1, 2]);
    }

    #[test]
    fn test_parse_mtl() {
        let mtl = "newmtl Red\nKd 1.0 0.0 0.0\nnewmtl Blue\nKd 0.0 0.0 1.0\n";
        let mut materials = HashMap::new();
        parse_mtl(mtl, &mut materials);
        assert_eq!(materials.len(), 2);
        assert!((materials["Red"].diffuse[0] - 1.0).abs() < 0.01);
        assert!((materials["Blue"].diffuse[2] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_import_obj_bytes() {
        let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let importer = ObjImporter::from_bytes(obj.to_vec());
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

    #[test]
    fn test_usemtl_groups() {
        let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 2 0 0\nv 2 1 0\nv 1 1 0\n\
                    usemtl Red\nf 1 2 3\nusemtl Blue\nf 4 5 6\n";
        let (_, groups, _) = parse_obj(obj).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].material, "Red");
        assert_eq!(groups[1].material, "Blue");
    }

    #[test]
    fn test_negative_indices() {
        let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\n";
        let (_, groups, _) = parse_obj(obj).unwrap();
        assert_eq!(groups[0].faces[0], vec![0, 1, 2]);
    }
}
