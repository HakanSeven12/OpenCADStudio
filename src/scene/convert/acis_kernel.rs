//! Drawing an ACIS solid by lifting it into the geometry kernel.
//!
//! A DWG or DXF carries its solids as ACIS records — analytic surfaces and the
//! topology joining them — and the kernel holds exactly that. So the shortest
//! path from a file to a picture is to lift the document into a `Body` and ask
//! the kernel for triangles, rather than to re-derive each surface's extent by
//! sampling it.
//!
//! # Why it can still fall short
//!
//! A face on a surface the kernel does not model, a curve it has no form for,
//! a pointer graph that does not hold together: [`lift`] reports each as a
//! [`Loss`] rather than quietly dropping it. What comes back then is a body
//! with faces missing, and the mesh it makes has holes — which is why the
//! result is marked incomplete and the caller keeps its own sampler for those.
//!
//! Saying so is the point. A partial mesh that claimed to be whole would show
//! a solid with a wall missing and nothing to suggest anything was wrong.

use acadrust::acis::lift;
use acadrust::entities::acis::SatDocument;
use acadrust::kernel::brep;

use crate::scene::convert::solid3d_tess::{body_transform, finalize_mesh};
use crate::scene::model::mesh_model::MeshLodSet;

/// How far a triangle may sit from the surface it lies on, as a fraction of
/// that surface's own radius.
///
/// A fraction rather than a length, because a length carries an assumption
/// about the drawing's units: a centimetre of sag is nothing on a pipeline
/// and is the whole of a bolt.
///
/// The *same* fraction the feature edges use, and deliberately so. Those edges
/// are drawn over these faces, so sampling the two differently leaves the wire
/// cutting across a facet instead of running along its corners — the rim of a
/// cylinder standing proud of the wall it bounds. Sharing the constant is what
/// keeps them from drifting apart when one is tuned.
use crate::scene::convert::solid3d_tess::EDGE_CHORD_FRAC as CHORD_FRAC;

/// What counts as the same point when the kernel reads a body over.
///
/// A micrometre, in a drawing measured in metres. Not slackness: an edge is
/// shared by two faces, and in a real file it cannot sit exactly on both,
/// because the two surfaces were fitted separately and written to finite
/// precision. Asked for exactness the kernel decides the edge is not on its
/// own plane, declines to project it, and the face is dropped — twenty-six
/// walls of one building went missing at a nanometre that no drawing means.
///
/// Loosening further buys almost nothing: a hundredth of this recovers one
/// more face in sixty thousand, and past that the tolerance would start
/// accepting geometry that really is wrong.
const TOL: f64 = 1e-6;

/// Tessellate an ACIS document by lifting it into the kernel.
///
/// `None` when nothing in the document lifts at all. The result's `complete`
/// flag says whether every face made it; a caller with a fallback sampler
/// uses it to decide whether to run one.
pub fn tessellate_sat(
    document: &SatDocument,
    name: String,
    color: [f32; 4],
    facet_res: f64,
) -> Option<MeshLodSet> {
    let (bodies, loss) = lift(document);
    if bodies.is_empty() {
        return None;
    }
    // `facet_res` is a resolution multiplier, not a length — the same one
    // `scale_lod` divides the fallback sampler's chord fraction by. Using it
    // as a sag made every solid as coarse as its own boundary: at the default
    // it asked for a whole world unit of departure, which on anything smaller
    // than that means no subdivision at all, and a pipe came out with as many
    // sides as its rim had points.
    //
    // It is not applied here at all. The feature edges these faces are drawn
    // under are built once at highest detail and never scaled, so scaling the
    // faces would pull the two apart again at any setting but one.
    let _ = facet_res;
    let frac = CHORD_FRAC;

    // Positions stay f64 until `finalize_mesh` splits them into the coarse
    // and fine pair, so a solid at survey coordinates keeps its millimetres.
    let mut positions: Vec<[f64; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    // A face the kernel holds but cannot express in its surface's own
    // parameters leaves a hole, the same as one that never lifted — so both
    // are counted before calling the mesh whole.
    let mut undrawn = 0usize;
    for body in &bodies {
        // What a flat face is sampled against: it never departs from its own
        // plane, so only its boundary arcs care, and the body's own size is
        // the nearest thing to a radius they have.
        let span = body_span(body);
        for face in body.face_keys() {
            let sag = frac * face_radius(body, face).unwrap_or(span);
            let Some(mesh) = brep::mesh::face(body, face, sag, TOL) else {
                undrawn += 1;
                continue;
            };
            let base = positions.len() as u32;
            positions.extend_from_slice(&mesh.positions);
            normals.extend(
                mesh.normals
                    .iter()
                    .map(|n| [n[0] as f32, n[1] as f32, n[2] as f32]),
            );
            indices.extend(
                mesh.triangles
                    .iter()
                    .flat_map(|t| [base + t[0] as u32, base + t[1] as u32, base + t[2] as u32]),
            );
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
        Vec::new(),
        Vec::new(),
        color,
        body_transform(document),
    ));
    set.complete = loss.is_empty() && undrawn == 0;
    Some(set)
}

/// The radius of the surface a face lies on, where it has one.
///
/// A torus is measured by its tube rather than its ring: the tube is the
/// tighter bend, and sampling to the ring would leave the section a hexagon.
fn face_radius(body: &brep::Body, face: brep::FaceKey) -> Option<f64> {
    let surface = body.surfaces.get(body.faces.get(face)?.surface)?;
    match surface {
        brep::Surface::Plane(_) => None,
        brep::Surface::Cylinder(cylinder) => Some(cylinder.radius),
        brep::Surface::Cone(cone) => Some(cone.radius),
        brep::Surface::Sphere(sphere) => Some(sphere.radius),
        brep::Surface::Torus(torus) => Some(torus.minor_radius),
    }
}

/// How big a body is, from the corners it is built on.
fn body_span(body: &brep::Body) -> f64 {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for (_, vertex) in body.vertices.iter() {
        for axis in 0..3 {
            low[axis] = low[axis].min(vertex.point[axis]);
            high[axis] = high[axis].max(vertex.point[axis]);
        }
    }
    if low[0] > high[0] {
        return 1.0;
    }
    (0..3)
        .map(|axis| high[axis] - low[axis])
        .fold(0.0_f64, f64::max)
        .max(1e-9)
}

/// The edges of every body in an ACIS document, as polylines.
///
/// What draws a solid's wireframe and what a click hit-tests against. Taken
/// from the kernel's own curves rather than from the mesh, so a rim is a
/// circle sampled to tolerance instead of whatever the triangulation left
/// along it.
///
/// Not called yet: the solid tessellator keeps its own feature-edge pass,
/// which also carries isolines. Here because it is the kernel's answer to the
/// same question, and the two should converge on it.
#[allow(dead_code)]
pub fn edge_polylines(document: &SatDocument, sag: f64) -> Vec<Vec<[f64; 3]>> {
    let (bodies, _) = lift(document);
    let sag = if sag > 0.0 { sag } else { CHORD_FRAC };
    let placement = body_transform(document);
    bodies
        .iter()
        .flat_map(|body| brep::edge_polylines(body, sag))
        .map(|polyline| {
            polyline
                .into_iter()
                .map(|point| placed(point, placement))
                .collect()
        })
        .collect()
}

/// A body-local point moved to where the body sits.
///
/// ACIS treats points as row vectors — `p' = scale·(p·M) + T` — so the stored
/// 3×3 is indexed transposed from a column-vector multiply. Getting that the
/// wrong way round mirrors a placed solid rather than moving it.
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

    /// How many sides a circle of `radius` gets at a sag of `frac × radius`.
    fn sides(frac: f64) -> f64 {
        let step = 2.0 * (1.0 - frac).clamp(-1.0, 1.0).acos();
        std::f64::consts::TAU / step
    }

    #[test]
    fn a_round_surface_gets_the_same_sides_whatever_its_size() {
        // The fault: `facet_res` is a resolution multiplier and was used as a
        // sag in world units. At the default that asked for a whole unit of
        // departure, so nothing smaller than a metre subdivided at all and a
        // pipe came out as coarse as its own rim.
        //
        // A fraction of the radius carries no unit, so a bolt and a pipeline
        // are sampled alike.
        assert!(sides(CHORD_FRAC) > 24.0, "{}", sides(CHORD_FRAC));
        assert!(sides(CHORD_FRAC) < 96.0, "{}", sides(CHORD_FRAC));
    }

    /// And it is the edges' own density, so the wire drawn over a face lands
    /// on the facet corners rather than cutting across them.
    #[test]
    fn a_face_is_sampled_as_finely_as_the_edges_over_it() {
        let rim = crate::scene::convert::solid3d_tess::edge_arc_segs(
            5.0,
            std::f64::consts::TAU,
        ) as f64;
        let wall = sides(CHORD_FRAC);
        assert!((rim - wall).abs() <= 1.0, "rim {rim} vs wall {wall}");
    }


}
