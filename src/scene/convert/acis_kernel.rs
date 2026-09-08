//! Drawing an ACIS solid by lifting it into the geometry kernel.
//!
//! A DWG or DXF carries its solids as ACIS records — analytic surfaces and the
//! topology joining them — and the kernel holds exactly that. So the shortest
//! path from a file to a picture is to lift the document into a `Body` and ask
//! the kernel for triangles, rather than to re-derive each surface's extent by
//! sampling it.
//!
//! Lift and tessellation failures are reported as an incomplete result.

use cadkernel::acis::lift;
use acadrust::entities::acis::SatDocument;
use cadkernel::brep;

use crate::scene::convert::solid3d_tess::{body_transform, finalize_mesh};
use crate::scene::model::mesh_model::{CurvedGen, MeshLodSet};

const DEFAULT_FIT_TOLERANCE: f64 = 1e-6;

/// Tessellate an ACIS document through the kernel.
pub fn tessellate_sat(
    document: &SatDocument,
    name: String,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: usize,
) -> Option<MeshLodSet> {
    let (bodies, loss) = lift(document);
    if bodies.is_empty() {
        return None;
    }
    let mut placed_bodies = Vec::with_capacity(bodies.len());
    for body in bodies {
        let source = body.provenance.source()?;
        let transform = body_transform(document, source.index() as usize).ok()?;
        let placement_scale = transform.map_or(1.0, |(_, _, scale)| scale.abs());
        let placed = if let Some((matrix, translation, scale)) = transform {
            let placement = brep::Placement {
                x_axis: [scale * matrix[0], scale * matrix[1], scale * matrix[2]],
                y_axis: [scale * matrix[3], scale * matrix[4], scale * matrix[5]],
                z_axis: [scale * matrix[6], scale * matrix[7], scale * matrix[8]],
                origin: translation,
            };
            brep::transform(&body, &placement)?
        } else {
            body
        };
        placed_bodies.push((placed, placement_scale));
    }
    let bodies = placed_bodies;
    let resolution = if facet_res.is_finite() && facet_res > 0.0 {
        facet_res.clamp(0.01, 10.0)
    } else {
        1.0
    };
    let max_angle = chordal_deflection.map_or_else(
        || cadkernel::tessellation::angle_for_resolution(resolution),
        |_| cadkernel::tessellation::display_angle_for_resolution(resolution),
    );

    // Positions stay f64 until `finalize_mesh` splits them into the coarse
    // and fine pair, so a solid at survey coordinates keeps its millimetres.
    let mut positions: Vec<[f64; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut edges: Vec<[f64; 3]> = Vec::new();
    let mut triangle_materials = Vec::new();
    let mut triangle_colors = Vec::new();
    let mut curved_gens = Vec::new();
    let face_materials: std::collections::HashMap<i32, acadrust::Handle> = document
        .records
        .iter()
        .filter(|record| record.entity_type == "material-adesk-attrib")
        .filter_map(|record| {
            let owner = record.token_pointer(2)?.0;
            let handle = record.token(3)?.as_integer()?;
            (owner >= 0 && handle > 0)
                .then(|| (owner, acadrust::Handle::new(handle as u64)))
        })
        .collect();
    let face_colors: std::collections::HashMap<i32, [f32; 4]> = document
        .records
        .iter()
        .filter(|record| record.entity_type == "color-adesk-attrib")
        .filter_map(|record| {
            let owner = record.token_pointer(2)?.0;
            let value = record.token(3)?.as_integer()?;
            let source = if (1..=255).contains(&value) {
                acadrust::Color::from_index(value as i16)
            } else if value > 257 {
                acadrust::Color::from_true_color_value(value as i32)
            } else {
                return None;
            };
            let mut rgba = crate::scene::convert::tess_util::aci_to_rgba(&source);
            rgba[3] = color[3];
            Some((owner, rgba))
        })
        .collect();
    // A face the kernel holds but cannot express in its surface's own
    // parameters leaves a hole, the same as one that never lifted — so both
    // are counted before calling the mesh whole.
    let mut undrawn = 0usize;
    let source_fit = if document.header.spatial_resolution.is_finite()
        && document.header.spatial_resolution > 0.0
    {
        document.header.spatial_resolution
    } else {
        DEFAULT_FIT_TOLERANCE
    };
    for (body, placement_scale) in &bodies {
        let mut tolerance = brep::mesh::TessellationTolerance::new(
            max_angle,
            source_fit * placement_scale,
        )
        .with_isolines(isolines);
        if let Some(deflection) = chordal_deflection {
            tolerance = tolerance.with_chordal_deflection(deflection);
        }
        let tessellation = brep::mesh::tessellate(body, tolerance);
        undrawn += tessellation.missing_faces.len();
        for face in &tessellation.triangle_faces {
            let record = body
                .faces
                .get(*face)
                .and_then(|face| face.provenance.source())
                .map(|source| source.index() as i32);
            triangle_materials.push(record.and_then(|record| face_materials.get(&record).copied()));
            triangle_colors.push(record.and_then(|record| face_colors.get(&record).copied()));
        }
        curved_gens.push(CurvedGen {
            source: tessellation.silhouette_source(),
        });
        let base = positions.len() as u32;
        positions.extend_from_slice(&tessellation.mesh.positions);
        normals.extend(
            tessellation
                .mesh
                .normals
                .iter()
                .map(|n| [n[0] as f32, n[1] as f32, n[2] as f32]),
        );
        indices.extend(tessellation.mesh.triangles.iter().flat_map(|triangle| {
            [
                base + triangle[0] as u32,
                base + triangle[1] as u32,
                base + triangle[2] as u32,
            ]
        }));
        for edge in tessellation.edges {
            for segment in edge.positions.windows(2) {
                edges.extend_from_slice(segment);
            }
        }
        for isoline in tessellation.isolines {
            for segment in isoline.positions.windows(2) {
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
        if let Some(properties) = brep::analytic_mass_properties(&single.0) {
            set.apply_mass_properties(properties);
        }
    }
    set.curved_gens = curved_gens;
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
