//! Convert a `SolidCore` B-rep into the neutral [`solidcore_record::step::Model`].
//!
//! This is the export counterpart of the STEP reader's build. It maps exact `SolidCore`
//! topology and analytic/B-spline geometry to the neutral model that
//! `step_export` writes, so an engine B-rep can be exported without being
//! tessellated.
//!
//! It writes a solid per material region: a `MANIFOLD_SOLID_BREP` of its outer shell,
//! or a `BREP_WITH_VOIDS` when the region has inner shells (voids), every face pointing
//! out of the material as ISO 10303-42 directs. Curves and surfaces are written as their exact analytic or
//! NURBS entities. Curved faces carry one pcurve per actual edge use, so a seam edge
//! used twice carries the two pcurves the writer joins into one `SEAM_CURVE`; a closed
//! edge used once (a cap circle) is no seam.

use std::collections::BTreeMap;

use solidcore::curve::curve::p1::ApproximateCurveOptions;
use solidcore::curve::curve::p2::CurveKind;
use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::ZONE_TOL_3D;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::topology::brep::outer_shells::outer_shells;
use solidcore::topology::brep::p2::is_sheet;
use solidcore::topology::edgeuse::p1 as eu;
use solidcore::topology::{Brep, VertexId};
use solidcore_record::step::model::{
    Body, BodyKind, Bound, Curve as ModelCurve, Edge, Face, Frame, Loop, Model, OrientedEdge,
    Shell, Surface as ModelSurface, Vertex,
};

/// Why a B-rep could not be mapped to the neutral model.
#[derive(Debug, Clone)]
pub struct ExportModelError {
    /// Human-readable reason.
    pub message: String,
}

impl std::fmt::Display for ExportModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExportModelError {}

fn err(message: impl Into<String>) -> ExportModelError {
    ExportModelError {
        message: message.into(),
    }
}

fn point(p: solidcore::math::Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn vec(v: solidcore::math::Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn unit(v: solidcore::math::Vec3) -> Result<[f64; 3], ExportModelError> {
    let raw = vec(v);
    let length = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
    if !(length.is_finite() && length > 0.0) {
        return Err(err("degenerate direction"));
    }
    Ok([raw[0] / length, raw[1] / length, raw[2] / length])
}

fn frame(
    position: &solidcore::geom::axis2_placement::Axis2Placement,
) -> Result<Frame, ExportModelError> {
    let z = unit(position.z_axis())?;
    let x = unit(position.x_axis())?;
    let y = [
        z[1] * x[2] - z[2] * x[1],
        z[2] * x[0] - z[0] * x[2],
        z[0] * x[1] - z[1] * x[0],
    ];
    Ok(Frame {
        origin: point(position.origin()),
        x,
        y,
        z,
    })
}

fn bspline_to_model(curve: &BSplineCurve) -> Result<ModelCurve, ExportModelError> {
    let (points, weights) = curve.control_polygon().map_err(|e| err(e.to_string()))?;
    let (knots, mults) = curve.knots(None).map_err(|e| err(e.to_string()))?;
    Ok(ModelCurve::BSpline(
        solidcore_record::step::model::BSplineCurve {
            degree: curve.degree().map_err(|e| err(e.to_string()))? as u32,
            points: points.into_iter().map(point).collect(),
            weights: (!weights.is_empty()).then_some(weights),
            knots,
            mults: mults.into_iter().map(|m| m as u32).collect(),
        },
    ))
}

fn approximate_crv_on_surf(
    kernel: &Kernel,
    curve: &Curve,
    interval: Extent1d,
    tolerance: f64,
) -> Result<ModelCurve, ExportModelError> {
    let breaks = [interval.min(), interval.max()];
    let tol = if tolerance > 0.0 {
        tolerance
    } else {
        ZONE_TOL_3D
    };
    let opts = ApproximateCurveOptions::default();
    let approx = curve
        .approximate_curve(kernel, &breaks, tol, &opts)
        .map_err(|e| err(format!("failed to approximate curve on surface: {e}")))?;
    match approx.curve.kind() {
        CurveKind::BSpline(curve) => bspline_to_model(curve),
        other => Err(err(format!(
            "expected B-spline approximation for curve on surface, got {other:?}"
        ))),
    }
}

fn curve(curve: &Curve) -> Result<ModelCurve, ExportModelError> {
    match curve.kind() {
        CurveKind::Line(line) => Ok(ModelCurve::Line {
            origin: point(line.line_point()),
            dir: unit(line.line_vector())?,
            magnitude: line.line_scale().abs(),
        }),
        CurveKind::Ellipse(ellipse) => {
            let frame = frame(ellipse.position())?;
            let x = ellipse.x_radius();
            let y = ellipse.y_radius();
            if (x - y).abs() <= 1.0e-12 * x.abs().max(y.abs()).max(1.0) {
                Ok(ModelCurve::Circle { frame, radius: x })
            } else {
                Ok(ModelCurve::Ellipse {
                    frame,
                    semi1: x,
                    semi2: y,
                })
            }
        }
        CurveKind::BSpline(curve) => bspline_to_model(curve),
        CurveKind::CrvOnSurf(_) => approximate_crv_on_surf(
            &Kernel::default(),
            curve,
            curve.natural_interval(),
            ZONE_TOL_3D,
        ),
        other => Err(err(format!("unsupported curve kind {other:?}"))),
    }
}

fn surface(
    surface: &solidcore::surface::surface::Surface,
) -> Result<ModelSurface, ExportModelError> {
    use solidcore::surface::surface::p2::SurfaceKind;
    match surface.kind() {
        SurfaceKind::Plane(plane) => Ok(ModelSurface::Plane {
            frame: frame(plane.position())?,
        }),
        SurfaceKind::Sphere(sphere) => Ok(ModelSurface::Sphere {
            frame: frame(sphere.base().position())?,
            radius: sphere.radius(),
        }),
        SurfaceKind::Torus(torus) => Ok(ModelSurface::Torus {
            frame: frame(torus.base().position())?,
            major: torus.major_radius(),
            minor: torus.minor_radius(),
        }),
        SurfaceKind::Cone(cone) => {
            // STEP's semi-angle is positive: a cone narrowing along its axis is the same cone
            // about the axis turned over (x kept, y and z reversed; its normal still points out).
            let mut frame = frame(cone.rev().position())?;
            let mut semi_angle_deg = cone.semi_angle_deg();
            if semi_angle_deg < 0.0 {
                semi_angle_deg = -semi_angle_deg;
                frame.y = frame.y.map(|v| -v);
                frame.z = frame.z.map(|v| -v);
            }
            Ok(ModelSurface::Cone { frame, radius: cone.base_radius(), semi_angle_deg })
        }
        SurfaceKind::SurfOfRevolution(revolution) => Ok(ModelSurface::Revolution {
            curve: Box::new(curve(
                revolution
                    .gen_curve()
                    .ok_or_else(|| err("revolution has no generator"))?,
            )?),
            axis_origin: point(revolution.position().origin()),
            axis_dir: unit(revolution.position().z_axis())?,
        }),
        SurfaceKind::SurfOfExtrusion(extrusion) => {
            let vector = extrusion.extrusion_vector();
            let magnitude =
                (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt();
            Ok(ModelSurface::Extrusion {
                curve: Box::new(curve(
                    extrusion
                        .gen_curve()
                        .ok_or_else(|| err("extrusion has no generator"))?,
                )?),
                dir: unit(vector)?,
                magnitude,
            })
        }
        SurfaceKind::BSpline(bspline) => bspline_surface(bspline),
        // the blend corner's ball patch (opt-in): its net, exactly on its sphere
        SurfaceKind::BallPatch(patch) => bspline_surface(patch.net()),
        other => Err(err(format!("unsupported surface kind {other:?}"))),
    }
}

/// A B-spline surface's STEP model: its control net, weights and knots.
fn bspline_surface(
    bspline: &solidcore::nurbs::b_spline_surface::BSplineSurface,
) -> Result<ModelSurface, ExportModelError> {
    let net = bspline
        .control_point_net()
        .map_err(|e| err(e.to_string()))?;
    let v_count = net.v_count.max(1);
    let points: Vec<Vec<[f64; 3]>> = net
        .points
        .chunks(v_count)
        .map(|row| row.iter().map(|p| point(*p)).collect())
        .collect();
    let weights = if net.weights.is_empty() {
        None
    } else {
        Some(net.weights.chunks(v_count).map(<[f64]>::to_vec).collect())
    };
    let canonical = bspline.canonical().map_err(|e| err(e.to_string()))?;
    Ok(ModelSurface::BSpline(
        solidcore_record::step::model::BSplineSurface {
            u_degree: canonical.u_degree as u32,
            v_degree: canonical.v_degree as u32,
            points,
            weights,
            u_knots: canonical.u_knots,
            u_mults: canonical
                .u_multiplicities
                .into_iter()
                .map(|m| m as u32)
                .collect(),
            v_knots: canonical.v_knots,
            v_mults: canonical
                .v_multiplicities
                .into_iter()
                .map(|m| m as u32)
                .collect(),
        },
    ))
}

/// Which of `surface`'s STEP parameters (u, v) are angles (ISO 10303-42): a cylinder's
/// or cone's u (its v is a length), a sphere's or torus's u and v, a surface of
/// revolution's u and, for a conic generator, its v (the generator's parameter), and an
/// extrusion's u for a conic generator.
fn step_angles(surface: &solidcore::surface::surface::Surface) -> (bool, bool) {
    use solidcore::surface::surface::p2::SurfaceKind;
    let conic = |curve: Option<&Curve>| {
        curve.is_some_and(|curve| matches!(curve.kind(), CurveKind::Ellipse(_)))
    };
    match surface.kind() {
        SurfaceKind::Cone(_) => (true, false),
        SurfaceKind::Sphere(_) | SurfaceKind::Torus(_) => (true, true),
        SurfaceKind::SurfOfRevolution(revolution) => (true, conic(revolution.gen_curve())),
        SurfaceKind::SurfOfExtrusion(extrusion) => (conic(extrusion.gen_curve()), false),
        _ => (false, false),
    }
}

/// Whether `surface`'s STEP parameterisation (ISO 10303-42, the one [`surface`] writes)
/// turns its normal over against the kernel's, ∂S/∂u × ∂S/∂v in its NURB parameters. The
/// written axis, radii and generator are the same either way; what differs is how the NURB
/// parameters run: swapped (the NURB's u is STEP's v), or one of them running against
/// STEP's (an inside-out cone's height from the top, sphere's latitude from the +90 pole,
/// torus's minor angle backwards). Each such reversal turns the normal over once.
fn reverses_normal(surface: &solidcore::surface::surface::Surface) -> bool {
    use solidcore::surface::surface::p2::SurfaceKind;
    match surface.kind() {
        SurfaceKind::Cone(cone) => {
            let canonical = cone.canonical();
            canonical.swap_uv ^ canonical.inside_out
        }
        SurfaceKind::Sphere(sphere) => sphere.base().swap_uv() ^ sphere.inside_out(),
        SurfaceKind::Torus(torus) => torus.base().swap_uv() ^ torus.inside_out(),
        SurfaceKind::SurfOfRevolution(revolution) => revolution.swap_uv(),
        SurfaceKind::SurfOfExtrusion(extrusion) => extrusion.swap_uv(),
        _ => false,
    }
}

/// Maps one B-rep to the neutral STEP model.
pub fn brep_to_model(brep: &Brep) -> Result<Model, ExportModelError> {
    let vertex_ids = brep.vertex_list().to_vec();
    let vertex_index: BTreeMap<VertexId, usize> = vertex_ids
        .iter()
        .enumerate()
        .map(|(i, v)| (*v, i))
        .collect();
    let vertices = vertex_ids
        .iter()
        .map(|id| {
            brep.vertex(*id)
                .map(|v| Vertex {
                    step_id: u64::from(id.index()) + 1,
                    point: point(v.point),
                })
                .map_err(|e| err(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let kernel = Kernel::default();
    let mut edges = Vec::new();
    let mut edge_index = BTreeMap::new();
    for id in brep.edge_list() {
        let edge = brep.edge(*id).map_err(|e| err(e.to_string()))?;
        let (start, end) = solidcore::topology::edge::start_end_vertices(brep, *id)
            .map_err(|e| err(e.to_string()))?;
        let start = start
            .and_then(|v| vertex_index.get(&v).copied())
            .ok_or_else(|| err("edge has no start vertex"))?;
        let end = end
            .and_then(|v| vertex_index.get(&v).copied())
            .ok_or_else(|| err("edge has no end vertex"))?;
        let edge_curve = edge
            .curve
            .as_ref()
            .ok_or_else(|| err("edge has no curve"))?;
        let interval = edge.interval;
        edge_index.insert(*id, edges.len());
        let curve_geom = match edge_curve.kind() {
            CurveKind::CrvOnSurf(_) => {
                approximate_crv_on_surf(&kernel, edge_curve, interval, edge.tolerance)?
            }
            _ => curve(edge_curve)?,
        };
        edges.push(Edge {
            step_id: u64::from(id.index()) + 1,
            start,
            end,
            curve: curve_geom,
            same_sense: true,
            t_start: interval.min(),
            t_end: interval.max(),
        });
    }

    let mut faces = Vec::new();
    let mut face_indices = Vec::new();
    let mut face_by_id = BTreeMap::new();
    let mut pcurves = BTreeMap::new();
    let pcurve_kernel = &kernel;
    // ISO 10303-42 wants a solid's faces written from outside its material: each face's
    // normal (the surface's, reversed where `same_sense` is false) pointing out of the
    // material, and its loops anticlockwise about that normal. A faceuse in a shell of a
    // material region faces into that region. A region's outer shell (`outer_shells`, the one
    // mated to the region enclosing it; the kernel's list order says nothing here):
    // each face of it is written from its mate, the faceuse on the outer side. Its other shells bound voids, each written as a BREP_WITH_VOIDS void: a
    // closed shell whose faces point out of the void's own volume (the material faceuses
    // themselves, pointing into the material), under an ORIENTED_CLOSED_SHELL of
    // orientation .F., which turns them out of the material.
    let mut written_faceuses = BTreeMap::new();
    let outer_of = outer_shells(brep).map_err(|e| err(e.to_string()))?;
    // each material region's shells, outer first
    let mut solids: Vec<Vec<solidcore::topology::ShellId>> = Vec::new();
    for region_id in brep.regions().ids() {
        let region = brep.region(region_id).map_err(|e| err(e.to_string()))?;
        if region.is_void() != 0 {
            continue;
        }
        let mut shells = region.shells.clone();
        if shells.len() > 1 {
            let outer = *outer_of.get(&region_id).ok_or_else(|| {
                err("a solid region with voids is not reached from the infinite region")
            })?;
            if !shells.contains(&outer) {
                return Err(err("a region's outer shell is not one of its shells"));
            }
            shells = std::iter::once(outer)
                .chain(shells.into_iter().filter(|s| *s != outer))
                .collect();
        }
        for (position, shell_id) in shells.iter().enumerate() {
            let shell = brep.shell(*shell_id).map_err(|e| err(e.to_string()))?;
            for faceuse_id in shell.get_faceuses().map_err(|e| err(e.to_string()))? {
                let faceuse = brep.faceuse(faceuse_id).map_err(|e| err(e.to_string()))?;
                if let Some(face) = faceuse.face {
                    let written = if position == 0 {
                        faceuse
                            .mate
                            .ok_or_else(|| err("a material faceuse has no mate"))?
                    } else {
                        faceuse_id
                    };
                    written_faceuses.insert(face, written);
                }
            }
        }
        solids.push(shells);
    }
    for id in brep.face_list() {
        let face = brep.face(*id).map_err(|e| err(e.to_string()))?;
        let surf = face
            .surface
            .as_ref()
            .ok_or_else(|| err("face has no surface"))?;
        let faceuse = written_faceuses
            .get(id)
            .copied()
            .or(face.faceuse)
            .ok_or_else(|| err("face has no faceuse"))?;
        let loops =
            eu::faceuse_loopuses(brep, faceuse, false, None).map_err(|e| err(e.to_string()))?;
        let mut bounds = Vec::new();
        for (loop_index, loopuse) in loops.iter().enumerate() {
            let edgeuses = eu::loopuse_edgeuses(brep, *loopuse).map_err(|e| err(e.to_string()))?;
            let mut oriented = Vec::new();
            let planar_face = matches!(
                face.surface
                    .as_ref()
                    .map(solidcore::surface::surface::Surface::kind),
                Some(solidcore::surface::surface::p2::SurfaceKind::Plane(_))
            );
            for (use_index, edgeuse) in edgeuses.into_iter().enumerate() {
                let edge = eu::edge(brep, edgeuse)
                    .map_err(|e| err(e.to_string()))?
                    .ok_or_else(|| err("edgeuse has no edge"))?;
                if !planar_face {
                    // The UV trim only enriches this face's boundary: without one the
                    // edge use still contributes its oriented edge below.
                    if let Ok((Some(uv_curve), owner)) = eu::uv_trim_curve(brep, edgeuse) {
                        let interval = uv_curve.natural_interval();
                        let (low, high) = (interval.min(), interval.max());
                        // The UV trim curve is in the surface's NURB parameters; a STEP pcurve
                        // is in its STEP ones (ISO 10303-42), which SolidCore converts to and
                        // from (the angles not linearly). A pcurve with a sample that does not
                        // convert is left out.
                        let mut uv = Vec::with_capacity(65);
                        let mut converted = true;
                        // OCS: a narrowing cone is written about its axis turned over
                        // ([`surface`]), where its angle and height both run backwards.
                        let turned = matches!(
                            surf.kind(),
                            solidcore::surface::surface::p2::SurfaceKind::Cone(cone) if cone.semi_angle_deg() < 0.0
                        );
                        for step in 0..=64 {
                            let t = low + (high - low) * f64::from(step) / 64.0;
                            if let Ok(p) = uv_curve.evaluate_point(pcurve_kernel, t) {
                                match surf.convert_uv_from_nurbs_to_step(
                                    pcurve_kernel,
                                    solidcore::geom::vector2d::Point2::new(p.x, p.y),
                                ) {
                                    Ok(step_uv) if turned => uv.push([-step_uv.x, -step_uv.y]),
                                    Ok(step_uv) => uv.push([step_uv.x, step_uv.y]),
                                    Err(_) => converted = false,
                                }
                            }
                        }
                        if !converted {
                            uv.clear();
                        }
                        if uv.len() >= 2 {
                            // A UV trim curve runs along the edge use that holds it, which
                            // is this use's mate when this use holds none: the pcurve runs
                            // along the edge.
                            let holder = owner.unwrap_or(edgeuse);
                            let opposite = eu::orientation(brep, holder).is_ok_and(|orientation| {
                                orientation
                                    != solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same
                            });
                            if opposite {
                                uv.reverse();
                            }
                            let mut curves = vec![uv];
                            // SolidCore's STEP parameters give angles in degrees; the file's
                            // plane angle unit is the radian ([`step_angles`]).
                            let (angular_u, angular_v) = step_angles(surf);
                            for curve in &mut curves {
                                for point in curve {
                                    if angular_u {
                                        point[0] = point[0].to_radians();
                                    }
                                    if angular_v {
                                        point[1] = point[1].to_radians();
                                    }
                                }
                            }
                            pcurves
                                .insert((u64::from(id.index()) + 1, loop_index, use_index), curves);
                        }
                    }
                }
                let forward = eu::orientation(brep, edgeuse).map_err(|e| err(e.to_string()))?
                    == solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same;
                oriented.push(OrientedEdge {
                    edge: *edge_index
                        .get(&edge)
                        .ok_or_else(|| err("edgeuse edge not indexed"))?,
                    forward,
                });
            }
            bounds.push(Loop {
                outer: loop_index == 0,
                // Edgeuses already traverse the oriented boundary, including
                // clockwise inner loops. Do not reverse that traversal again.
                orientation: true,
                bound: Bound::Edges(oriented),
            });
        }
        // The faceuse's side is the kernel surface's normal (its NURB parameters'), or its
        // reverse; `same_sense` states it against the STEP surface's normal
        // ([`reverses_normal`]). The loops stay as the faceuse directs them.
        let same_sense = (brep
            .faceuse(faceuse)
            .map_err(|e| err(e.to_string()))?
            .orientation
            == solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same)
            ^ reverses_normal(surf);
        face_indices.push(faces.len());
        face_by_id.insert(*id, faces.len());
        faces.push(Face {
            step_id: u64::from(id.index()) + 1,
            surface: surface(surf)?,
            same_sense,
            loops: bounds,
        });
    }
    if faces.is_empty() {
        return Err(err("B-rep has no faces"));
    }
    let mut kernel = Kernel::default();
    let sheet = is_sheet(&mut kernel, brep).map_err(|e| err(e.to_string()))?;
    // the faces of a shell, as indices into `faces`
    let shell_faces =
        |shell_id: solidcore::topology::ShellId| -> Result<Vec<usize>, ExportModelError> {
            let mut out = Vec::new();
            for faceuse in brep
                .shell(shell_id)
                .map_err(|e| err(e.to_string()))?
                .get_faceuses()
                .map_err(|e| err(e.to_string()))?
            {
                let face_id = brep.faceuse(faceuse).map_err(|e| err(e.to_string()))?.face;
                if let Some(index) = face_id.and_then(|face_id| face_by_id.get(&face_id)) {
                    out.push(*index);
                }
            }
            Ok(out)
        };
    let body = |index: usize, kind: BodyKind, shells: Vec<Shell>| Body {
        step_id: index as u64 + 1,
        name: None,
        kind,
        unit_mm: 1.0,
        uncertainty_mm: None,
        shells,
        faces: faces.clone(),
        edges: edges.clone(),
        vertices: vertices.clone(),
    };
    let mut bodies = Vec::new();
    if !sheet {
        // a solid per material region: its outer shell, and its voids
        for shells in &solids {
            let mut records = Vec::new();
            for (position, shell_id) in shells.iter().enumerate() {
                let faces_of = shell_faces(*shell_id)?;
                if faces_of.is_empty() {
                    continue;
                }
                records.push(Shell {
                    step_id: u64::from(shell_id.index()) + 1,
                    closed: true,
                    orientation: position == 0,
                    faces: faces_of,
                });
            }
            if records.first().is_some_and(|outer| outer.orientation) {
                let kind = if records.len() > 1 {
                    BodyKind::SolidWithVoids
                } else {
                    BodyKind::ManifoldSolid
                };
                bodies.push(body(bodies.len(), kind, records));
            }
        }
    }
    if bodies.is_empty() {
        // No material region (a sheet, or no regions): a body per distinct face shell.
        let mut seen_shell_faces: Vec<Vec<usize>> = Vec::new();
        let kind = if sheet {
            BodyKind::ShellModel
        } else {
            BodyKind::ManifoldSolid
        };
        for shell_id in brep.shells().ids() {
            let faces_of = shell_faces(shell_id)?;
            if faces_of.is_empty() {
                continue;
            }
            let mut canonical = faces_of.clone();
            canonical.sort_unstable();
            if seen_shell_faces.contains(&canonical) {
                continue;
            }
            seen_shell_faces.push(canonical);
            let shell = Shell {
                step_id: u64::from(shell_id.index()) + 1,
                closed: !sheet,
                orientation: true,
                faces: faces_of,
            };
            bodies.push(body(bodies.len(), kind, vec![shell]));
        }
    }
    if bodies.is_empty() {
        // No shell: every live face in one body.
        let shell = Shell {
            step_id: 1,
            closed: !sheet,
            orientation: true,
            faces: face_indices.clone(),
        };
        let kind = if sheet {
            BodyKind::ShellModel
        } else {
            BodyKind::ManifoldSolid
        };
        bodies.push(body(0, kind, vec![shell]));
    }
    Ok(Model {
        bodies,
        pcurves,
        ..Model::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::kernel::Kernel;
    use solidcore::math::Point3;
    use solidcore::primitive::api_primitives as api;

    /// How many times each face's boundary uses each edge, and how many sampled pcurve
    /// curves the model gives that (face, `BRep` edge): `(uses, curves)`.
    fn pcurves_by_face_edge(model: &Model) -> BTreeMap<(u64, usize), (usize, usize)> {
        let body = &model.bodies[0];
        let mut counts = BTreeMap::new();
        for face in &body.faces {
            for (loop_index, loop_) in face.loops.iter().enumerate() {
                let Bound::Edges(edges) = &loop_.bound else {
                    continue;
                };
                for (use_index, oriented) in edges.iter().enumerate() {
                    let entry = counts
                        .entry((face.step_id, oriented.edge))
                        .or_insert((0, 0));
                    entry.0 += 1;
                    if let Some(curves) = model.pcurves.get(&(face.step_id, loop_index, use_index))
                    {
                        entry.1 += curves.len();
                    }
                }
            }
        }
        counts
    }

    #[test]
    fn maps_a_box_and_a_sphere_to_the_neutral_model() {
        let mut kernel = Kernel::default();
        for brep in [
            api::create_box(&mut kernel, Point3::new(-1.0, -1.0, -1.0), 2.0, 2.0, 2.0).unwrap(),
            api::create_sphere(&mut kernel, Point3::new(0.0, 0.0, 0.0), 2.0).unwrap(),
        ] {
            let model = brep_to_model(&brep).expect("brep to model");
            assert_eq!(model.bodies.len(), 1);
            let body = &model.bodies[0];
            assert!(!body.faces.is_empty());
            assert!(!body.edges.is_empty());
            assert!(!body.vertices.is_empty());
            assert!(body.faces.iter().any(|face| matches!(
                face.surface,
                ModelSurface::Plane { .. } | ModelSurface::Sphere { .. }
            )));
        }
        let cylinder =
            api::create_cylinder(&mut kernel, Point3::new(0.0, 0.0, -2.0), 2.0, 4.0).unwrap();
        let model = brep_to_model(&cylinder).expect("cylinder to model");
        // One curve per actual edge use: the wall's seam edge is used twice, each cap
        // circle once. A closed cap circle is not a seam.
        let grouped = pcurves_by_face_edge(&model);
        for (&(face, edge), &(uses, curves)) in &grouped {
            assert!(
                curves == uses || curves == 0,
                "face {face} edge {edge}: {uses} uses carry {curves} curves: {:?}",
                model.pcurves
            );
        }
        let seams: Vec<_> = grouped.iter().filter(|(_, &(uses, _))| uses == 2).collect();
        assert_eq!(
            seams.len(),
            1,
            "the cylinder has one seam edge (its wall's): {grouped:?}"
        );
        let (seam, &(seam_uses, seam_curves)) = seams[0];
        assert_eq!(
            (seam_uses, seam_curves),
            (2, 2),
            "seam {seam:?} must carry one curve per use: {grouped:?}"
        );
        let cap_trims: Vec<_> = grouped
            .iter()
            .filter(|(_, &(uses, curves))| uses == 1 && curves > 0)
            .collect();
        assert_eq!(
            cap_trims.len(),
            2,
            "the wall trims two cap circles, one use each: {grouped:?}"
        );
        for (key, &(uses, curves)) in cap_trims {
            assert_eq!(
                (uses, curves),
                (1, 1),
                "cap trim {key:?} must carry one curve: {grouped:?}"
            );
        }
        for curves in model.pcurves.values() {
            for curve in curves {
                assert!(
                    curve
                        .iter()
                        .any(|point| point[0].abs() <= std::f64::consts::TAU + 0.5),
                    "cylinder pcurve u must be radians: {curve:?}"
                );
            }
        }
        // The cylinder's v is a length: the seam's pcurves span the 4 mm height.
        let vs = model
            .pcurves
            .values()
            .flatten()
            .flatten()
            .map(|point| point[1]);
        let (low, high) = vs.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), v| {
            (low.min(v), high.max(v))
        });
        assert!(
            (high - low - 4.0).abs() < 1e-9,
            "cylinder pcurve v must be millimetres: {low} to {high}"
        );
    }

    fn plus(p: [f64; 3], s: f64, d: [f64; 3]) -> [f64; 3] {
        [p[0] + s * d[0], p[1] + s * d[1], p[2] + s * d[2]]
    }

    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// The point of a line or circle at its file parameter (a circle's in radians).
    fn iso_curve_point(curve: &ModelCurve, t: f64) -> [f64; 3] {
        match curve {
            ModelCurve::Line {
                origin,
                dir,
                magnitude,
            } => plus(*origin, t * magnitude, *dir),
            ModelCurve::Circle { frame, radius } => plus(
                plus(frame.origin, radius * t.cos(), frame.x),
                radius * t.sin(),
                frame.y,
            ),
            other => panic!("generator {other:?}"),
        }
    }

    /// The ISO 10303-42 point of an exported surface at file parameters (angles in
    /// radians).
    fn iso_point(surface: &ModelSurface, u: f64, v: f64) -> [f64; 3] {
        let (su, cu) = u.sin_cos();
        let (sv, cv) = v.sin_cos();
        let around = |frame: &Frame, r: f64, height: f64| {
            plus(
                plus(plus(frame.origin, r * cu, frame.x), r * su, frame.y),
                height,
                frame.z,
            )
        };
        match surface {
            ModelSurface::Cylinder { frame, radius } => around(frame, *radius, v),
            ModelSurface::Cone {
                frame,
                radius,
                semi_angle_deg,
            } => around(frame, radius + v * semi_angle_deg.to_radians().tan(), v),
            ModelSurface::Sphere { frame, radius } => around(frame, radius * cv, radius * sv),
            ModelSurface::Torus {
                frame,
                major,
                minor,
            } => around(frame, major + minor * cv, minor * sv),
            ModelSurface::Revolution {
                curve,
                axis_origin,
                axis_dir,
            } => {
                // The generator's point rotated by u about the axis.
                let d = plus(iso_curve_point(curve, v), -1.0, *axis_origin);
                let along = dot(d, *axis_dir);
                let radial = plus(d, -along, *axis_dir);
                let turned = plus(plus([0.0; 3], cu, radial), su, cross(*axis_dir, radial));
                plus(plus(*axis_origin, along, *axis_dir), 1.0, turned)
            }
            ModelSurface::Extrusion {
                curve,
                dir,
                magnitude,
            } => plus(iso_curve_point(curve, u), v * magnitude, *dir),
            other => panic!("surface {other:?}"),
        }
    }

    /// Every exported pcurve point, lifted by the ISO 10303-42 formula of the exported
    /// surface at the file's parameters, lies on an edge of the B-rep: the pcurves are
    /// in STEP parameters (converted from the kernel's NURB ones), angles in radians.
    #[test]
    fn exported_pcurves_lie_on_their_edges() {
        use solidcore::curve::ellipse::p2::EllipseOptions;
        use solidcore::curve::ellipse::Ellipse;
        use solidcore::curve::line::Line;
        use solidcore::geom::extent2d::Extent2d;
        use solidcore::geom::global_solver::SolverOperation;
        use solidcore::math::Vec3;
        use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
        use solidcore::surface::surf_of_revolution::SurfOfRevolution;
        use solidcore::surface::surface::Surface;
        use solidcore::topology::brep::p2::edgeuses;
        use solidcore::topology::brep::sheet::sheet_on;
        use solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve;
        let mut kernel = Kernel::default();
        let origin = Point3::new(0.0, 0.0, 0.0);
        let mut breps = vec![
            (
                "cylinder",
                api::create_cylinder(&mut kernel, Point3::new(0.0, 0.0, -2.0), 2.0, 4.0).unwrap(),
            ),
            (
                "cone",
                api::create_cone(&mut kernel, origin, 3.0, 1.0, 4.0).unwrap(),
            ),
            (
                "sphere",
                api::create_sphere(&mut kernel, origin, 2.0).unwrap(),
            ),
            (
                "torus",
                api::create_torus(&mut kernel, origin, 5.0, 1.5).unwrap(),
            ),
        ];
        // A surface of revolution of a line and an extrusion of a circle (u an angle
        // on both), each a one-face sheet: 20 mm tall at radius 10 about the z axis.
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let mut line =
            Line::from_points(Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 0.0, 20.0), 3);
        // SurfOfRevolution::new revolves the generator's NURB form
        line.make_nurb().unwrap();
        let revolved_line = SurfOfRevolution::new(
            &kernel,
            Curve::from(line),
            origin,
            x,
            y,
            Extent2d::from_bounds(0.0, 0.0, 360.0, 1.0),
            false,
        )
        .unwrap();
        let circle = Ellipse::new_circle(
            &kernel,
            origin,
            x,
            y,
            Extent1d::from_min_max(0.0, 360.0),
            10.0,
            &EllipseOptions {
                dim: 3,
                ..EllipseOptions::default()
            },
        )
        .unwrap();
        let extruded_circle = SurfOfExtrusion::new(
            &kernel,
            Curve::from(circle),
            origin,
            Vec3::new(0.0, 0.0, 20.0),
            Extent2d::from_bounds(0.0, 0.0, 360.0, 1.0),
            false,
        )
        .unwrap();
        let sheets = [
            ("revolved line", Surface::from(revolved_line)),
            ("extruded circle", Surface::from(extruded_circle)),
        ];
        for (name, surface) in sheets {
            let (mut brep, _) =
                sheet_on(&mut kernel, surface, 1e-6).unwrap_or_else(|e| panic!("{name}: {e}"));
            // the sheet's sides have no UV curves yet: each edge use gets its own
            // (no distance or deviation wanted, a UV-space curve is fine)
            for edgeuse in edgeuses(&brep, None).unwrap() {
                get_or_create_uv_trim_curve(&kernel, &mut brep, edgeuse, false, false, true)
                    .unwrap()
                    .status
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
            }
            // the face keeps its kind, so the checks below go through its branch
            let model = brep_to_model(&brep).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                model.bodies[0].faces.iter().all(|face| matches!(
                    (name, &face.surface),
                    ("revolved line", ModelSurface::Revolution { .. })
                        | ("extruded circle", ModelSurface::Extrusion { .. })
                )),
                "{name}: exported as {:?}",
                model.bodies[0].faces
            );
            breps.push((name, brep));
        }
        for (name, brep) in &breps {
            let model = brep_to_model(brep).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!model.pcurves.is_empty(), "{name}: no pcurves");
            // (a degenerate edge, e.g. at a pole, may have no curve)
            let edges: Vec<_> = brep
                .edge_list()
                .iter()
                .filter_map(|&e| {
                    let edge = brep.edge(e).unwrap();
                    Some((edge.curve.as_ref()?, edge.interval))
                })
                .collect();
            for ((face_id, _, _), curves) in &model.pcurves {
                let face = model.bodies[0]
                    .faces
                    .iter()
                    .find(|face| face.step_id == *face_id)
                    .unwrap();
                for &[u, v] in curves.iter().flatten() {
                    let [x, y, z] = iso_point(&face.surface, u, v);
                    let p = Point3::new(x, y, z);
                    let gap = edges
                        .iter()
                        .filter_map(|(curve, interval)| {
                            curve
                                .drop_point(
                                    &kernel,
                                    interval,
                                    p,
                                    None,
                                    1e-9,
                                    None,
                                    SolverOperation::Minimize,
                                )
                                .ok()
                                .filter(|dropped| dropped.success)
                                .map(|dropped| dropped.distance)
                        })
                        .fold(f64::INFINITY, f64::min);
                    assert!(gap < 1e-4, "{name}: ({u}, {v}) lifts {gap} off every edge");
                }
            }
        }
    }
}
