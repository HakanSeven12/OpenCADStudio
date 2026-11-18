//! Drawing an ACIS solid by reading it into Solidcore records and meshing them.
//!
//! A DWG or DXF carries its solids as ACIS records — analytic surfaces and the topology joining
//! them — and a Solidcore record holds exactly that, so the shortest path from a file to a
//! picture is to read the document into records and ask Solidcore's mesher for triangles.
//!
//! Read and mesh failures are reported as an incomplete result.

use document::entities::acis::SatDocument;
use ocs_solid::lift;

use crate::scene::convert::solid3d_tess::finalize_mesh;
use crate::scene::model::mesh_model::{CurvedGen, MeshLodSet};
use crate::scene::model::silhouette::SilhouetteSource;
use crate::scene::model::solid_model::{self, DisplayTolerance};

/// Tessellate an ACIS document through Solidcore.
pub fn tessellate_sat(
    document: &SatDocument,
    name: String,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: [usize; 2],
    planar_isolines: Option<bool>,
) -> Option<MeshLodSet> {
    let (bodies, loss) = lift(document);
    if bodies.is_empty() {
        return None;
    }
    let max_angle = match chordal_deflection {
        Some(_) => solid_model::display_angle_for_resolution(facet_res),
        None => solid_model::angle_for_resolution(facet_res),
    };

    // Positions stay f64 until `finalize_mesh` splits them into the coarse and fine pair, so a
    // solid at survey coordinates keeps its millimetres.
    let mut positions: Vec<[f64; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut edges: Vec<[f64; 3]> = Vec::new();
    let mut edge_colors: Vec<Option<[f32; 4]>> = Vec::new();
    let mut triangle_materials = Vec::new();
    let mut triangle_colors = Vec::new();
    let mut curved_gens = Vec::new();
    let face_materials: std::collections::HashMap<i32, document::Handle> = document
        .records
        .iter()
        .filter(|record| record.entity_type == "material-adesk-attrib")
        .filter_map(|record| {
            let owner = record.token_pointer(2)?.0;
            let handle = record.token(3)?.as_integer()?;
            (owner >= 0 && handle > 0).then(|| (owner, document::Handle::new(handle as u64)))
        })
        .collect();
    // A true colour attribute outranks the index one beside it, which only
    // holds its nearest index.
    let face_colors: std::collections::HashMap<i32, [f32; 4]> = document
        .records
        .iter()
        .filter(|record| record.entity_type == "color-adesk-attrib")
        .chain(
            document
                .records
                .iter()
                .filter(|record| record.entity_type == "truecolor-adesk-attrib"),
        )
        .filter_map(|record| {
            let owner = record.token_pointer(2)?.0;
            let mut value = record.token(3)?.as_integer()?;
            if record.entity_type == "truecolor-adesk-attrib" {
                value = (value & 0x00FF_FFFF) | 0x0100_0000;
            }
            let source = if (1..=255).contains(&value) {
                document::Color::from_index(value as i16)
            } else if value > 257 {
                document::Color::from_true_color_value(value as i32)
            } else {
                return None;
            };
            let mut rgba = crate::scene::convert::tess_util::aci_to_rgba(&source);
            rgba[3] = color[3];
            Some((owner, rgba))
        })
        .collect();
    // A face Solidcore cannot mesh leaves a hole, the same as one that never read — so both
    // are counted before calling the mesh whole.
    let mut undrawn = 0usize;
    for body in &bodies {
        let tolerance = DisplayTolerance::new(max_angle)
            .with_isolines(isolines, planar_isolines)
            .with_chord(chordal_deflection);
        let Some(mesh) = solid_model::tessellate(&body.brep, tolerance) else {
            undrawn += body.brep.faces.len().max(1);
            continue;
        };
        undrawn += mesh.missing_faces.len();
        for face in &mesh.triangle_faces {
            let record = body.face_records.get(*face as usize).map(|r| *r as i32);
            triangle_materials.push(record.and_then(|record| face_materials.get(&record).copied()));
            triangle_colors.push(record.and_then(|record| face_colors.get(&record).copied()));
        }
        curved_gens.push(CurvedGen {
            source: SilhouetteSource::of_mesh(&mesh.positions, &mesh.normals, &mesh.triangles, &mesh.triangle_faces),
        });
        let base = positions.len() as u32;
        positions.extend_from_slice(&mesh.positions);
        normals.extend(mesh.normals.iter().map(|n| n.map(|v| v as f32)));
        indices.extend(mesh.triangles.iter().flat_map(|t| t.map(|i| base + i as u32)));
        for (_, points) in &mesh.edges {
            for segment in points.windows(2) {
                edges.extend_from_slice(segment);
                edge_colors.push(None);
                edge_colors.push(None);
            }
        }
        for (_, points) in &mesh.isolines {
            for segment in points.windows(2) {
                edges.extend_from_slice(segment);
            }
        }
    }
    if indices.is_empty() {
        return None;
    }

    // ACIS keeps a body's geometry in its own local frame and records where it
    // sits in a separate `transform` record. Skipping that leaves every solid
    // stacked at the origin — which is what a BIM file looks like when each
    // component is placed rather than authored in world coordinates.
    let mut set = MeshLodSet::from_single(finalize_mesh(
        name,
        positions,
        normals,
        indices,
        triangle_materials,
        triangle_colors,
        color,
        None,
    ));
    if let [single] = bodies.as_slice() {
        if ocs_solid::model::is_solid(&single.brep) {
            if let Ok(estimate) = ocs_solid::model::mass_properties(&single.brep) {
                set.apply_mass_properties(crate::scene::model::mass::MassProperties::from_estimate(&estimate));
            }
        }
    }
    set.curved_gens = curved_gens;
    if edge_colors.iter().any(Option::is_some) {
        set.edge_vert_colors = edge_colors;
    }
    for point in edges {
        let high = [point[0] as f32, point[1] as f32, point[2] as f32];
        set.edge_verts.push(high);
        set.edge_verts_low.push([
            (point[0] - high[0] as f64) as f32,
            (point[1] - high[1] as f64) as f32,
            (point[2] - high[2] as f64) as f32,
        ]);
    }
    set.complete = loss.is_empty() && undrawn == 0;
    set.build_display_lods();
    Some(set)
}

/// A body-local point moved to where the body sits.
///
/// ACIS treats points as row vectors — `p' = scale·(p·M) + T` — so the stored
/// 3×3 is indexed transposed from a column-vector multiply. Getting that the
/// wrong way round mirrors a placed solid rather than moving it.
#[cfg(test)]
fn placed(point: [f64; 3], xform: Option<([f64; 9], [f64; 3], f64)>) -> [f64; 3] {
    let Some((m, translation, scale)) = xform else {
        return point;
    };
    let [x, y, z] = point;
    [
        scale * (x * m[0] + y * m[3] + z * m[6]) + translation[0],
        scale * (x * m[1] + y * m[4] + z * m[7]) + translation[1],
        scale * (x * m[2] + y * m[5] + z * m[8]) + translation[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter turn about z, written the way ACIS writes it: row-major, and
    /// applied to points as row vectors.
    fn quarter_turn() -> Option<([f64; 9], [f64; 3], f64)> {
        Some((
            [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            [10.0, 20.0, 30.0],
            2.0,
        ))
    }

    #[test]
    fn a_placement_turns_the_way_acis_means_it_to() {
        // Deliberately asymmetric: a transposed multiply turns the other way,
        // so this catches the one mistake the convention invites.
        let moved = placed([1.0, 0.0, 0.0], quarter_turn());
        assert!((moved[0] - 10.0).abs() < 1e-12, "{moved:?}");
        assert!((moved[1] - 22.0).abs() < 1e-12, "{moved:?}");
        assert!((moved[2] - 30.0).abs() < 1e-12, "{moved:?}");
    }

    #[test]
    fn a_body_with_no_transform_stays_where_it_is() {
        // Many solids store absolute geometry and carry no transform record.
        // Treating that as anything but identity moves them off their own
        // coordinates.
        let point = [3.0, -4.0, 5.0];
        assert_eq!(placed(point, None), point);
    }

    #[test]
    fn the_scale_reaches_the_translation_only_once() {
        // `p' = scale·(p·M) + T`: the translation is not scaled. Folding the
        // scale into it as well puts a placed solid at twice its offset,
        // which reads as a plausible position and is the wrong one.
        let moved = placed([0.0, 0.0, 0.0], quarter_turn());
        assert_eq!(moved, [10.0, 20.0, 30.0]);
    }
}
