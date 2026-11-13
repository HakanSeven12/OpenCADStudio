// Wavefront OBJ mesh importer.
//
// Each OBJ object (`o`) becomes one MESH entity holding its own vertices and
// its polygons as they are in the file, so the drawing keeps, snaps, saves
// and plots it like any other MESH.

use document::entities::mesh::{Mesh, MeshFace};
use document::types::Vector3;

/// Parse OBJ text into one MESH per object.
/// Returns `None` if the file has no usable geometry.
pub fn parse_obj(src: &str) -> Option<Vec<Mesh>> {
    let mut positions: Vec<Vector3> = Vec::new();
    // Each object's faces as file vertex indices.
    let mut objects: Vec<Vec<Vec<usize>>> = vec![Vec::new()];

    for line in src.lines() {
        let line = line.trim();
        let mut parts = line.split_whitespace();
        match parts.next().unwrap_or("") {
            "v" => {
                let vals: Vec<f64> = parts.filter_map(|s| s.parse().ok()).collect();
                if vals.len() >= 3 {
                    // OBJ is Y-up right-handed; OpenCADStudio's world is Z-up.
                    // Rotate +90° about X so OBJ Y→world Z (up): (x, y, z) → (x, -z, y).
                    positions.push(Vector3::new(vals[0], -vals[2], vals[1]));
                }
            }
            // An object with no faces yet is simply continued.
            "o" if objects.last().is_some_and(|faces| !faces.is_empty()) => {
                objects.push(Vec::new());
            }
            "f" => {
                // Vertex descriptors "v", "v/vt", "v/vt/vn", "v//vn"; negative
                // indices count back from the vertices read so far.
                let face: Option<Vec<usize>> = parts
                    .map(|token| {
                        let index: i64 = token.split('/').next()?.parse().ok()?;
                        let index = if index < 0 {
                            positions.len() as i64 + index
                        } else {
                            index - 1
                        };
                        usize::try_from(index)
                            .ok()
                            .filter(|&index| index < positions.len())
                    })
                    .collect();
                if let Some(mut face) = face {
                    face.dedup();
                    if face.len() > 1 && face.first() == face.last() {
                        face.pop();
                    }
                    if face.len() >= 3 {
                        objects.last_mut()?.push(face);
                    }
                }
            }
            _ => {}
        }
    }

    let meshes: Vec<Mesh> = objects
        .into_iter()
        .filter(|faces| !faces.is_empty())
        .map(|faces| {
            // Each mesh keeps only the vertices its faces use.
            let mut local = std::collections::HashMap::new();
            let mut mesh = Mesh::new();
            for face in faces {
                let indices = face
                    .into_iter()
                    .map(|index| {
                        *local
                            .entry(index)
                            .or_insert_with(|| mesh.add_vertex(positions[index]))
                    })
                    .collect();
                mesh.add_face(MeshFace::new(indices));
            }
            mesh.compute_edges();
            mesh
        })
        .collect();
    (!meshes.is_empty()).then_some(meshes)
}

#[cfg(test)]
mod tests {
    use super::parse_obj;

    #[test]
    fn each_object_becomes_a_mesh_of_its_own_polygons() {
        let src = "v 0 0 0\nv 1 0 0\nv 1 0 -1\nv 0 0 -1\nv 0 1 0\n\
                   o Floor\nf 1//1 2//1 3//1 4//1\no Roof\nf -5 -4 -1\n";
        let meshes = parse_obj(src).unwrap();
        assert_eq!(meshes.len(), 2);
        let floor = &meshes[0];
        assert_eq!(floor.vertices.len(), 4);
        assert_eq!(floor.faces[0].vertices, vec![0, 1, 2, 3]);
        // OBJ Y-up lands as world Z-up.
        assert_eq!(floor.vertices[2].y, 1.0);
        let roof = &meshes[1];
        assert_eq!(roof.vertices.len(), 3);
        assert_eq!(roof.vertices[2].z, 1.0);
    }
}
