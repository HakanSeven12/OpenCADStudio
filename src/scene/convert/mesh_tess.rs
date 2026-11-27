//! How a mesh draws shaded: its faces as a level-of-detail set with normals
//! and feature edges.

use crate::entities::mesh::*;

fn make_mesh_lod_set(
    name: String,
    color: [f32; 4],
    vertices: Vec<[f64; 3]>,
    faces: Vec<Vec<usize>>,
    visible_edges: std::collections::HashSet<(usize, usize)>,
    face_colors: &[Option<[f32; 4]>],
) -> Option<crate::scene::model::mesh_model::MeshLodSet> {
    let visible_edges = crease_edges(&vertices, &faces, visible_edges);
    let (indices, triangle_faces) = face_triangle_indices(&vertices, &faces);
    if indices.is_empty() {
        return None;
    }
    let normals = vertex_normals(&vertices, &indices);
    let triangle_colors = triangle_faces
        .iter()
        .map(|&face| face_colors.get(face).copied().flatten())
        .collect();
    let model = super::solid3d_tess::finalize_mesh(
        name,
        vertices.clone(),
        normals,
        indices,
        Vec::new(),
        triangle_colors,
        color,
        None,
    );
    let mut set = crate::scene::model::mesh_model::MeshLodSet::from_lods(vec![model]);
    for (a, b) in visible_edges {
        append_feature_edge(
            &mut set.edge_verts,
            &mut set.edge_verts_low,
            &vertices,
            a,
            b,
        );
    }
    Some(set)
}

/// Convert every standard DWG mesh family to the material-aware shaded mesh
/// pipeline. The retained wire entity carries snaps/grips only; fill, normals,
/// depth, materials and face colours live in `MeshLodSet`.
pub(crate) fn tessellate_shaded_mesh(
    entity: &document::EntityType,
    color: [f32; 4],
) -> Option<crate::scene::model::mesh_model::MeshLodSet> {
    use std::collections::HashSet;

    match entity {
        document::EntityType::Mesh(mesh) => {
            let display = display_mesh(mesh);
            let mut edges = HashSet::new();
            for face in &display.faces {
                for corner in 0..face.len() {
                    edges.insert(edge_key(face[corner], face[(corner + 1) % face.len()]));
                }
            }
            make_mesh_lod_set(
                mesh.common.handle.value().to_string(),
                color,
                display.vertices,
                display.faces,
                edges,
                &[],
            )
        }
        document::EntityType::PolygonMesh(mesh) => {
            let (vertices, m, n) = polygon_mesh_display(mesh);
            if m == 0 || n == 0 || vertices.len() < m.saturating_mul(n) {
                return None;
            }
            let closed_m = mesh.is_closed_m();
            let closed_n = mesh.is_closed_n();
            let m_cells = if closed_m { m } else { m.saturating_sub(1) };
            let n_cells = if closed_n { n } else { n.saturating_sub(1) };
            let mut faces = Vec::with_capacity(m_cells.saturating_mul(n_cells));
            let mut edges = HashSet::new();
            for i in 0..m_cells {
                for j in 0..n_cells {
                    let face = vec![
                        i * n + j,
                        ((i + 1) % m) * n + j,
                        ((i + 1) % m) * n + (j + 1) % n,
                        i * n + (j + 1) % n,
                    ];
                    for corner in 0..4 {
                        edges.insert(edge_key(face[corner], face[(corner + 1) % 4]));
                    }
                    faces.push(face);
                }
            }
            make_mesh_lod_set(
                mesh.common.handle.value().to_string(),
                color,
                vertices,
                faces,
                edges,
                &[],
            )
        }
        document::EntityType::PolyfaceMesh(mesh) => {
            let vertices: Vec<[f64; 3]> = mesh
                .vertices
                .iter()
                .map(|vertex| [vertex.location.x, vertex.location.y, vertex.location.z])
                .collect();
            let mut faces = Vec::new();
            let mut face_colors = Vec::new();
            let mut edges = HashSet::new();
            for face in &mesh.faces {
                let raw = [face.index1, face.index2, face.index3, face.index4];
                let indices: Vec<usize> = raw
                    .iter()
                    .filter(|&&index| index != 0)
                    .filter_map(|&index| (index.unsigned_abs() as usize).checked_sub(1))
                    .filter(|&index| index < vertices.len())
                    .collect();
                if indices.len() < 3 {
                    continue;
                }
                for corner in 0..indices.len() {
                    if raw[corner] > 0 {
                        edges.insert(edge_key(
                            indices[corner],
                            indices[(corner + 1) % indices.len()],
                        ));
                    }
                }
                faces.push(indices);
                face_colors.push(
                    face.color
                        .as_ref()
                        .map(crate::display::tess_util::aci_to_rgba),
                );
            }
            make_mesh_lod_set(
                mesh.common.handle.value().to_string(),
                color,
                vertices,
                faces,
                edges,
                &face_colors,
            )
        }
        _ => None,
    }
}
