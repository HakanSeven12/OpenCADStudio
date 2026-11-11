use document::entities::EmbeddedEntity;
use document::objects::{
    SolidHistoryBoolean, SolidHistoryLoft, SolidHistoryOperation, SolidHistoryRevolve,
    SolidHistorySweep, SolidHistoryTree,
};
use document::types::{Matrix3, Vector3};

use kernel::brep::{self, Body, Placement};
use kernel::geom2d::{
    Arc, Circle, Curve, Ellipse, EllipseArc, Line, NurbsCurve, Parameterization, Polyline,
    PolylineVertex,
};
use kernel::space::{coplanarity_tolerance, NurbsCurve3, PlanarCurve, Plane, Vec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRebuildError {
    Unsupported,
    InvalidParameters,
    InvalidTransform,
    InvalidBrep,
    Fillet(brep::FilletError),
    Chamfer(brep::ChamferError),
    /// A sweep the record asks for that a cornered path refuses.
    Refused(brep::SweepRefusal),
    /// The two solids a boolean step joins could not be combined.
    Boolean,
}

impl std::fmt::Display for HistoryRebuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => formatter.write_str("unsupported solid history operation"),
            Self::InvalidParameters => formatter.write_str("invalid solid history parameters"),
            Self::InvalidTransform => formatter.write_str("invalid solid history transform"),
            Self::InvalidBrep => formatter.write_str("invalid solid history B-rep"),
            Self::Fillet(error) => write!(formatter, "solid history fillet failed: {error}"),
            Self::Chamfer(error) => write!(formatter, "solid history chamfer failed: {error}"),
            Self::Refused(why) => write!(formatter, "sweep refused: {why:?} along a path with a corner"),
            Self::Boolean => formatter.write_str("solid history boolean failed"),
        }
    }
}

impl std::error::Error for HistoryRebuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Fillet(error) => Some(error),
            Self::Chamfer(error) => Some(error),
            _ => None,
        }
    }
}

impl From<brep::FilletError> for HistoryRebuildError {
    fn from(error: brep::FilletError) -> Self {
        Self::Fillet(error)
    }
}

impl From<brep::ChamferError> for HistoryRebuildError {
    fn from(error: brep::ChamferError) -> Self {
        Self::Chamfer(error)
    }
}

fn placement(matrix: [f64; 16]) -> Result<Placement, HistoryRebuildError> {
    if matrix.iter().any(|value| !value.is_finite())
        || matrix[3].abs() > 1e-9
        || matrix[7].abs() > 1e-9
        || matrix[11].abs() > 1e-9
        || (matrix[15] - 1.0).abs() > 1e-9
    {
        return Err(HistoryRebuildError::InvalidTransform);
    }
    Ok(Placement {
        x_axis: [matrix[0], matrix[1], matrix[2]],
        y_axis: [matrix[4], matrix[5], matrix[6]],
        z_axis: [matrix[8], matrix[9], matrix[10]],
        origin: [matrix[12], matrix[13], matrix[14]],
    })
}

fn finish(
    body: Option<Body>,
    transform: [f64; 16],
) -> Result<Body, HistoryRebuildError> {
    let body = body.ok_or(HistoryRebuildError::InvalidParameters)?;
    brep::transform(&body, &placement(transform)?)
        .ok_or(HistoryRebuildError::InvalidTransform)
}

fn ocs_plane(normal: Vector3, elevation: f64) -> Result<Plane, HistoryRebuildError> {
    let normal = Vec3::from([normal.x, normal.y, normal.z])
        .normalize()
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    let normal = Vector3::new(normal.x, normal.y, normal.z);
    let axes = Matrix3::arbitrary_axis(normal);
    Ok(Plane::from_axes(
        [
            normal.x * elevation,
            normal.y * elevation,
            normal.z * elevation,
        ],
        [axes.m[0][0], axes.m[1][0], axes.m[2][0]],
        [axes.m[0][1], axes.m[1][1], axes.m[2][1]],
    ))
}

fn straight_curve(
    start: Vector3,
    end: Vector3,
) -> Result<PlanarCurve, HistoryRebuildError> {
    let start = [start.x, start.y, start.z];
    let end = [end.x, end.y, end.z];
    let direction = Vec3::from(end) - Vec3::from(start);
    if direction.length_squared() <= 1e-24 {
        return Err(HistoryRebuildError::InvalidParameters);
    }
    let plane = if (end[2] - start[2]).abs() <= 1e-9 * direction.length().max(1.0) {
        Plane::from_axes([0.0, 0.0, start[2]], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
    } else {
        let normal = direction.cross(Vec3::Z).normalize().unwrap_or(Vec3::Y);
        Plane::orthonormal(start, direction.to_array(), normal.to_array())
            .ok_or(HistoryRebuildError::InvalidParameters)?
    };
    let start = plane
        .project(start)
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    let end = plane
        .project(end)
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    Ok(PlanarCurve::new(plane, Curve::Line(Line { start, end })))
}

fn spline_curve(
    value: &document::entities::Spline,
) -> Result<PlanarCurve, HistoryRebuildError> {
    let degree = value.degree.max(1) as usize;
    let fit_method = !value.fit_points.is_empty() && value.control_points.len() <= degree;
    let source = if fit_method {
        &value.fit_points
    } else {
        &value.control_points
    };
    let first = source
        .first()
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    let normal = Vec3::from([value.normal.x, value.normal.y, value.normal.z])
        .normalize()
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    let elevation = Vec3::from([first.x, first.y, first.z]).dot(normal);
    let plane = ocs_plane(
        Vector3::new(normal.x, normal.y, normal.z),
        elevation,
    )?;
    let source_points = source
        .iter()
        .map(|point| [point.x, point.y, point.z])
        .collect::<Vec<_>>();
    let tolerance = coplanarity_tolerance(&source_points);
    if !tolerance.is_finite()
        || !source_points
            .iter()
            .all(|point| plane.contains(*point, tolerance))
    {
        return Err(HistoryRebuildError::InvalidParameters);
    }
    let point = |value: &Vector3| {
        plane
            .project([value.x, value.y, value.z])
            .ok_or(HistoryRebuildError::InvalidParameters)
    };
    let nurbs = if fit_method {
        if !value.flags.periodic {
            for tangent in [value.begin_tangent, value.end_tangent] {
                let tangent = Vec3::from([tangent.x, tangent.y, tangent.z]);
                if tangent.length_squared() > 1e-18
                    && tangent.dot(normal).abs() > 1e-9 * tangent.length().max(1.0)
                {
                    return Err(HistoryRebuildError::InvalidParameters);
                }
            }
        }
        let mut points = value
            .fit_points
            .iter()
            .map(point)
            .collect::<Result<Vec<_>, _>>()?;
        let parameterization = match value.knot_parameterization {
            2 => Parameterization::Uniform,
            1 => Parameterization::Centripetal,
            _ => Parameterization::Chord,
        };
        if value.flags.periodic {
            NurbsCurve::interpolate_periodic(&points, parameterization)
        } else {
            if value.flags.closed && points.first() != points.last() {
                points.push(points[0]);
            }
            let tangent = |value: Vector3| {
                let projected = plane.project_vector([value.x, value.y, value.z])?;
                (projected[0].hypot(projected[1]) > 1e-9).then_some(projected)
            };
            NurbsCurve::interpolate(
                &points,
                tangent(value.begin_tangent),
                tangent(value.end_tangent),
                parameterization,
            )
        }
    } else {
        NurbsCurve::new(
            degree,
            value
                .control_points
                .iter()
                .map(point)
                .collect::<Result<Vec<_>, _>>()?,
            value.knots.clone(),
            (!value.weights.is_empty()).then(|| value.weights.clone()),
        )
    }
    .ok_or(HistoryRebuildError::InvalidParameters)?;
    Ok(PlanarCurve::new(plane, Curve::Nurbs(nurbs)))
}

/// The spans of a polyline the modeler keeps as a wire body.
fn body_wire_spans(entity: &EmbeddedEntity) -> Option<Vec<crate::WireSpan>> {
    let EmbeddedEntity::Body { acis_data, .. } = entity else {
        return None;
    };
    crate::lift_wire(&acis_data.parse()?)
}

/// A planar wire body as a bulged polyline in its own plane: lines and
/// circular arcs, open or closed.
fn wire_polyline(spans: &[crate::WireSpan]) -> Result<PlanarCurve, HistoryRebuildError> {
    let points = spans
        .iter()
        .flat_map(|span| [span.start, span.end])
        .collect::<Vec<_>>();
    let first = Vec3::from(spans.first().ok_or(HistoryRebuildError::InvalidParameters)?.start);
    let arc_normal = spans.iter().find_map(|span| match &span.curve {
        brep::Curve3::Circle(circle) => circle.plane.normal(),
        _ => None,
    });
    let normal = match arc_normal {
        Some(normal) => Vec3::from(normal),
        // Newell's normal of the vertex chain.
        None => (0..points.len()).fold(Vec3::ZERO, |sum, index| {
            let a = Vec3::from(points[index]) - first;
            let b = Vec3::from(points[(index + 1) % points.len()]) - first;
            sum + a.cross(b)
        }),
    }
    .normalize()
    .ok_or(HistoryRebuildError::InvalidParameters)?;
    let x_axis = points
        .iter()
        .map(|point| Vec3::from(*point) - first)
        .map(|chord| chord - normal * chord.dot(normal))
        .find(|chord| chord.length() > 1e-12)
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    let plane = Plane::orthonormal(first.to_array(), x_axis.to_array(), normal.to_array())
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    let tolerance = coplanarity_tolerance(&points);
    if points.iter().any(|point| !plane.contains(*point, tolerance)) {
        return Err(HistoryRebuildError::Unsupported);
    }
    let mut vertices = Vec::with_capacity(spans.len() + 1);
    for span in spans {
        let bulge = match &span.curve {
            brep::Curve3::Line(_) => 0.0,
            brep::Curve3::Circle(circle) => {
                let axis = Vec3::from(circle.plane.normal().ok_or(HistoryRebuildError::InvalidParameters)?);
                let axis = if span.along_curve { axis } else { -axis };
                let centre = Vec3::from(circle.plane.origin);
                let (a, b) = (Vec3::from(span.start) - centre, Vec3::from(span.end) - centre);
                let mut sweep = a.cross(b).dot(axis).atan2(a.dot(b));
                if sweep <= 1e-12 {
                    sweep += std::f64::consts::TAU;
                }
                if axis.dot(normal).abs() < 1.0 - 1e-9 || sweep >= std::f64::consts::TAU - 1e-9 {
                    return Err(HistoryRebuildError::Unsupported);
                }
                (sweep / 4.0).tan() * axis.dot(normal).signum()
            }
            _ => return Err(HistoryRebuildError::Unsupported),
        };
        vertices.push(PolylineVertex {
            position: plane.project(span.start).ok_or(HistoryRebuildError::InvalidParameters)?,
            bulge,
        });
    }
    let last = spans.last().ok_or(HistoryRebuildError::InvalidParameters)?.end;
    let closed = Vec3::from(last).distance(first) <= tolerance.max(1e-9);
    if !closed {
        vertices.push(PolylineVertex {
            position: plane.project(last).ok_or(HistoryRebuildError::InvalidParameters)?,
            bulge: 0.0,
        });
    }
    Ok(PlanarCurve::new(plane, Curve::Polyline(Polyline { vertices, closed })))
}

fn embedded_curve(entity: &EmbeddedEntity) -> Result<PlanarCurve, HistoryRebuildError> {
    match entity {
        EmbeddedEntity::Line(value) => straight_curve(value.start, value.end),
        EmbeddedEntity::Circle(value) => Ok(PlanarCurve::new(
            ocs_plane(value.normal, value.center.z)?,
            Curve::Circle(Circle {
                centre: [value.center.x, value.center.y],
                radius: value.radius,
            }),
        )),
        EmbeddedEntity::Arc(value) => Ok(PlanarCurve::new(
            ocs_plane(value.normal, value.center.z)?,
            Curve::Arc(Arc {
                centre: [value.center.x, value.center.y],
                radius: value.radius,
                start_angle: value.start_angle,
                end_angle: value.end_angle,
            }),
        )),
        EmbeddedEntity::Ellipse(value) => {
            let normal = Vec3::from([value.normal.x, value.normal.y, value.normal.z])
                .normalize()
                .ok_or(HistoryRebuildError::InvalidParameters)?;
            let center = [value.center.x, value.center.y, value.center.z];
            let elevation = Vec3::from(center).dot(normal);
            let plane = ocs_plane(
                Vector3::new(normal.x, normal.y, normal.z),
                elevation,
            )?;
            let centre = plane
                .project(center)
                .ok_or(HistoryRebuildError::InvalidParameters)?;
            let major = plane
                .project_vector([
                    value.major_axis.x,
                    value.major_axis.y,
                    value.major_axis.z,
                ])
                .ok_or(HistoryRebuildError::InvalidParameters)?;
            let major_radius = major[0].hypot(major[1]);
            if !major_radius.is_finite()
                || major_radius <= 0.0
                || !value.minor_axis_ratio.is_finite()
                || value.minor_axis_ratio <= 0.0
            {
                return Err(HistoryRebuildError::InvalidParameters);
            }
            Ok(PlanarCurve::new(
                plane,
                Curve::Ellipse(EllipseArc {
                    ellipse: Ellipse {
                        centre,
                        major_radius,
                        minor_radius: major_radius * value.minor_axis_ratio,
                        major_axis: [major[0] / major_radius, major[1] / major_radius],
                    },
                    start_parameter: value.start_parameter,
                    end_parameter: value.end_parameter,
                }),
            ))
        }
        EmbeddedEntity::Body { .. } => wire_polyline(
            &body_wire_spans(entity).ok_or(HistoryRebuildError::InvalidParameters)?,
        ),
        EmbeddedEntity::Spline(value) => spline_curve(value),
        EmbeddedEntity::LwPolyline(value) => {
            if value.vertices.len() < 2 {
                return Err(HistoryRebuildError::InvalidParameters);
            }
            Ok(PlanarCurve::new(
                ocs_plane(value.normal, value.elevation)?,
                Curve::Polyline(Polyline {
                    vertices: value
                        .vertices
                        .iter()
                        .map(|vertex| PolylineVertex {
                            position: [vertex.location.x, vertex.location.y],
                            bulge: vertex.bulge,
                        })
                        .collect(),
                    closed: value.is_closed,
                }),
            ))
        }
        _ => Err(HistoryRebuildError::Unsupported),
    }
}

fn placed_curve(
    mut curve: PlanarCurve,
    transform: [f64; 16],
) -> Result<PlanarCurve, HistoryRebuildError> {
    let placement = placement(transform)?;
    if placement.scale().is_none() {
        return Err(HistoryRebuildError::InvalidTransform);
    }
    curve.plane = Plane::from_axes(
        placement.point(curve.plane.origin),
        placement.vector(curve.plane.x_axis),
        placement.vector(curve.plane.y_axis),
    );
    Ok(curve)
}

fn profile_pieces(curve: &Curve) -> Result<Vec<Curve>, HistoryRebuildError> {
    let pieces = match curve {
        Curve::Polyline(value) if value.closed => curve.segments(),
        Curve::Circle(value) => {
            (0..4)
                .map(|part| {
                    let start = std::f64::consts::FRAC_PI_2 * part as f64;
                    Curve::Arc(Arc {
                        centre: value.centre,
                        radius: value.radius,
                        start_angle: start,
                        end_angle: start + std::f64::consts::FRAC_PI_2,
                    })
                })
                .collect()
        }
        Curve::Arc(value) if curve.is_closed() => {
            let start = value.start_angle;
            let step = value.sweep() / 4.0;
            (0..4)
                .map(|part| {
                    Curve::Arc(Arc {
                        centre: value.centre,
                        radius: value.radius,
                        start_angle: start + step * part as f64,
                        end_angle: start + step * (part + 1) as f64,
                    })
                })
                .collect()
        }
        Curve::Ellipse(value) if curve.is_closed() => {
            let start = value.start_parameter;
            let step = value.sweep() / 4.0;
            (0..4)
                .map(|part| {
                    Curve::Ellipse(EllipseArc {
                        ellipse: value.ellipse,
                        start_parameter: start + step * part as f64,
                        end_parameter: start + step * (part + 1) as f64,
                    })
                })
                .collect()
        }
        _ => return Err(HistoryRebuildError::InvalidParameters),
    };
    (pieces.len() >= 3)
        .then_some(pieces)
        .ok_or(HistoryRebuildError::InvalidParameters)
}

fn path_pieces(curve: &Curve) -> Result<Vec<Curve>, HistoryRebuildError> {
    let pieces = match curve {
        Curve::Line(value) => vec![Curve::Line(*value)],
        Curve::Arc(value) => vec![Curve::Arc(*value)],
        Curve::Circle(value) => (0..4)
            .map(|part| {
                let start = std::f64::consts::FRAC_PI_2 * part as f64;
                Curve::Arc(Arc {
                    centre: value.centre,
                    radius: value.radius,
                    start_angle: start,
                    end_angle: start + std::f64::consts::FRAC_PI_2,
                })
            })
            .collect(),
        Curve::Polyline(_) => curve.segments(),
        Curve::Ellipse(value) => {
            let scale = value
                .ellipse
                .major_radius
                .max(value.ellipse.minor_radius)
                .max(1.0);
            let points = curve.tessellate_within(scale * 1e-4);
            points
                .windows(2)
                .filter(|pair| pair[0] != pair[1])
                .map(|pair| {
                    Curve::Line(Line {
                        start: pair[0],
                        end: pair[1],
                    })
                })
                .collect()
        }
        Curve::Nurbs(_) => {
            let points = curve.tessellate_within(1e-4);
            points
                .windows(2)
                .filter(|pair| pair[0] != pair[1])
                .map(|pair| {
                    Curve::Line(Line {
                        start: pair[0],
                        end: pair[1],
                    })
                })
                .collect()
        }
        _ => return Err(HistoryRebuildError::Unsupported),
    };
    (!pieces.is_empty())
        .then_some(pieces)
        .ok_or(HistoryRebuildError::InvalidParameters)
}

fn legacy_sweep(value: &SolidHistorySweep) -> Result<Body, HistoryRebuildError> {
    if !value.scale_factor.is_finite()
        || value.scale_factor <= 1e-9
        || !value.draft_angle.is_finite()
        || !value.twist_angle.is_finite()
        || !value.align_angle.is_finite()
    {
        return Err(HistoryRebuildError::Unsupported);
    }
    let profile = placed_curve(
        embedded_curve(
            value
                .sweep_entity
                .as_ref()
                .ok_or(HistoryRebuildError::InvalidParameters)?,
        )?,
        value.sweep_entity_transform,
    )?;
    let profile_pieces = profile_pieces(&profile.curve)?;
    let path_entity = value
        .path_entity
        .as_ref()
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    if let Some(points) = embedded_line_path_3d(path_entity) {
        if value.align_angle.abs() > 1e-12
            || value.twist_angle.abs() > 1e-12
            || (value.scale_factor - 1.0).abs() > 1e-12
        {
            return Err(HistoryRebuildError::Unsupported);
        }
        let placement = placement(value.path_entity_transform)?;
        let points = points
            .into_iter()
            .map(|point| placement.point(point))
            .collect::<Vec<_>>();
        let body = if value.draft_angle.abs() > 1e-12 {
            if points.len() != 2 {
                return Err(HistoryRebuildError::Unsupported);
            }
            #[cfg(feature = "offset")]
            {
                brep::extrude_tapered(
                    profile.plane,
                    &profile_pieces,
                    (Vec3::from(points[1]) - Vec3::from(points[0])).to_array(),
                    value.draft_angle,
                )
            }
            #[cfg(not(feature = "offset"))]
            {
                return Err(HistoryRebuildError::Unsupported);
            }
        } else {
            brep::sweep_along_polyline3d(profile.plane, &profile_pieces, &points)
        };
        return finish(body, value.base.transform);
    }
    let mut path = placed_curve(
        embedded_curve(path_entity)?,
        value.path_entity_transform,
    )?;
    let path_pieces = path_pieces(&path.curve)?;
    let profile_center = profile_pieces
        .iter()
        .map(|piece| Vec3::from(profile.plane.point_at(piece.point_at(0.0))))
        .fold(Vec3::ZERO, |sum, point| sum + point)
        / profile_pieces.len() as f64;
    let path_start = Vec3::from(path.plane.point_at(path.curve.point_at(0.0)));
    path.plane.origin = (Vec3::from(path.plane.origin) + profile_center - path_start).to_array();
    if value.draft_angle.abs() > 1e-12 {
        if value.align_angle.abs() > 1e-12
            || value.twist_angle.abs() > 1e-12
            || (value.scale_factor - 1.0).abs() > 1e-12
            || path_pieces.len() != 1
        {
            return Err(HistoryRebuildError::Unsupported);
        }
        let Curve::Line(line) = path_pieces[0] else {
            return Err(HistoryRebuildError::Unsupported);
        };
        let direction = Vec3::from(path.plane.point_at(line.end))
            - Vec3::from(path.plane.point_at(line.start));
        #[cfg(feature = "offset")]
        {
            return finish(
                brep::extrude_tapered(
                    profile.plane,
                    &profile_pieces,
                    direction.to_array(),
                    value.draft_angle,
                ),
                value.base.transform,
            );
        }
        #[cfg(not(feature = "offset"))]
        {
            return Err(HistoryRebuildError::Unsupported);
        }
    }
    finish(
        brep::sweep_along_deformed(
            profile.plane,
            &profile_pieces,
            path.plane,
            &path_pieces,
            value.align_angle,
            value.twist_angle,
            value.scale_factor,
        ),
        value.base.transform,
    )
}

fn sweep_profile_pieces(curve: &Curve) -> Result<Vec<Curve>, HistoryRebuildError> {
    let pieces = match curve {
        Curve::Polyline(_) => curve.segments(),
        Curve::Circle(_) => profile_pieces(curve)?,
        Curve::Arc(_) | Curve::Ellipse(_) if curve.is_closed() => profile_pieces(curve)?,
        Curve::Nurbs(value) if curve.is_closed() => (0..4)
            .map(|part| {
                value
                    .trimmed(part as f64 / 4.0, (part + 1) as f64 / 4.0)
                    .map(Curve::Nurbs)
                    .ok_or(HistoryRebuildError::InvalidParameters)
            })
            .collect::<Result<Vec<_>, _>>()?,
        Curve::Line(_) | Curve::Arc(_) | Curve::Ellipse(_) | Curve::Nurbs(_) => {
            vec![curve.clone()]
        }
        _ => return Err(HistoryRebuildError::Unsupported),
    };
    (!pieces.is_empty())
        .then_some(pieces)
        .ok_or(HistoryRebuildError::InvalidParameters)
}

fn region_spline_pcurve(
    body: &Body,
    key: brep::CoedgeKey,
    plane: Plane,
    tolerance: f64,
) -> Result<Option<Curve>, HistoryRebuildError> {
    let coedge = body.coedges.get(key).ok_or(HistoryRebuildError::InvalidBrep)?;
    if coedge.pcurve.is_some() {
        return Ok(None);
    }
    let edge = body.edges.get(coedge.edge).ok_or(HistoryRebuildError::InvalidBrep)?;
    let curve = body.curves.get(edge.curve).ok_or(HistoryRebuildError::InvalidBrep)?;
    let (degree, controls, knots, weights, from, to) = match curve {
        brep::Curve3::PlanarSpline { plane: source, curve } => (
            curve.degree(),
            curve.control_points().iter().map(|point| source.point_at(*point)).collect::<Vec<_>>(),
            curve.knots().to_vec(),
            curve.weights().to_vec(),
            edge.start_parameter,
            edge.end_parameter,
        ),
        brep::Curve3::Nurbs(curve) => {
            let (start, end) = curve.domain();
            (
                curve.degree(),
                curve.control_points().to_vec(),
                curve.knots().to_vec(),
                curve.weights().to_vec(),
                (edge.start_parameter - start) / (end - start),
                (edge.end_parameter - start) / (end - start),
            )
        }
        _ => return Ok(None),
    };
    if controls.iter().any(|point| !plane.contains(*point, tolerance)) {
        return Err(HistoryRebuildError::InvalidParameters);
    }
    let points = controls
        .iter()
        .map(|point| plane.project(*point).ok_or(HistoryRebuildError::InvalidParameters))
        .collect::<Result<Vec<_>, _>>()?;
    let curve = NurbsCurve::new(degree, points, knots, Some(weights))
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    let curve = if coedge.forward { curve.trimmed(from, to) } else { curve.trimmed(to, from) }
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    Ok(Some(Curve::Nurbs(curve)))
}

fn region_sweep_profile(
    region: &document::entities::Region,
) -> Result<(Plane, Vec<Vec<Curve>>), HistoryRebuildError> {
    if region.acis_data.has_data() {
        let document = region.acis_data.parse().ok_or(HistoryRebuildError::InvalidBrep)?;
        let (mut bodies, loss) = super::lift(&document);
        if bodies.len() != 1 || !loss.is_empty() {
            return Err(HistoryRebuildError::Unsupported);
        }
        let mut body = bodies.pop().ok_or(HistoryRebuildError::InvalidBrep)?;
        let mut faces = body.faces.iter();
        let (face_key, face) = faces.next().ok_or(HistoryRebuildError::InvalidBrep)?;
        if faces.next().is_some() {
            return Err(HistoryRebuildError::Unsupported);
        }
        let face = face.clone();
        let Some(brep::Surface::Plane(plane)) = body.surfaces.get(face.surface) else {
            return Err(HistoryRebuildError::InvalidParameters);
        };
        let plane = *plane;
        let tolerance = brep::operation_tolerance(&[&body]);
        let coedges = face.loops.iter().map(|key| {
            body.loops.get(*key).map(|ring| ring.coedges.clone())
                .ok_or(HistoryRebuildError::InvalidBrep)
        }).collect::<Result<Vec<_>, _>>()?.into_iter().flatten().collect::<Vec<_>>();
        for key in coedges {
            if let Some(curve) = region_spline_pcurve(&body, key, plane, tolerance)? {
                body.coedges.get_mut(key).ok_or(HistoryRebuildError::InvalidBrep)?.pcurve = Some(curve);
            }
        }
        let boundary = brep::pcurve::face_boundary_parts(&body, face_key, tolerance)
            .ok_or(HistoryRebuildError::Unsupported)?;
        let wires = face
            .loops
            .iter()
            .map(|key| {
                let ring = body.loops.get(*key).ok_or(HistoryRebuildError::InvalidBrep)?;
                let mut wire = Vec::new();
                for key in &ring.coedges {
                    let (_, curve) = boundary
                        .iter()
                        .find(|(candidate, _)| candidate == key)
                        .ok_or(HistoryRebuildError::InvalidBrep)?;
                    wire.extend(sweep_profile_pieces(curve)?);
                }
                (!wire.is_empty())
                    .then_some(wire)
                    .ok_or(HistoryRebuildError::InvalidBrep)
            })
            .collect::<Result<Vec<_>, _>>()?;
        return (!wires.is_empty())
            .then_some((plane, wires))
            .ok_or(HistoryRebuildError::InvalidBrep);
    }

    let point_wires = region
        .wires
        .iter()
        .map(|wire| {
            let mut points = wire
                .points
                .iter()
                .map(|point| [point.x, point.y, point.z])
                .collect::<Vec<_>>();
            if points.len() > 2 && points.first() == points.last() {
                points.pop();
            }
            (points.len() >= 3)
                .then_some(points)
                .ok_or(HistoryRebuildError::InvalidParameters)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let first = point_wires.first().ok_or(HistoryRebuildError::InvalidParameters)?;
    let origin = Vec3::from(first[0]);
    let along = Vec3::from(first[1]) - origin;
    let normal = first[2..]
        .iter()
        .find_map(|point| along.cross(Vec3::from(*point) - origin).normalize())
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    let plane = Plane::orthonormal(first[0], along.to_array(), normal.to_array())
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    let all_points = point_wires.iter().flatten().copied().collect::<Vec<_>>();
    let tolerance = coplanarity_tolerance(&all_points);
    if !tolerance.is_finite() || all_points.iter().any(|point| !plane.contains(*point, tolerance)) {
        return Err(HistoryRebuildError::InvalidParameters);
    }
    let wires = point_wires
        .into_iter()
        .map(|points| {
            let points = points
                .into_iter()
                .map(|point| plane.project(point).ok_or(HistoryRebuildError::InvalidParameters))
                .collect::<Result<Vec<_>, _>>()?;
            Ok((0..points.len())
                .map(|index| Curve::Line(Line {
                    start: points[index],
                    end: points[(index + 1) % points.len()],
                }))
                .collect())
        })
        .collect::<Result<Vec<_>, HistoryRebuildError>>()?;
    Ok((plane, wires))
}

/// Resolves an embedded sweep profile without discarding region holes or curves.
pub fn sweep_profile_geometry(
    entity: &EmbeddedEntity,
    transform: [f64; 16],
) -> Result<(Plane, Vec<Vec<Curve>>, bool), HistoryRebuildError> {
    if let EmbeddedEntity::Region(region) = entity {
        let (mut plane, wires) = region_sweep_profile(region)?;
        let place = placement(transform)?;
        if place.scale().is_none() {
            return Err(HistoryRebuildError::InvalidTransform);
        }
        plane = Plane::from_axes(
            place.point(plane.origin),
            place.vector(plane.x_axis),
            place.vector(plane.y_axis),
        );
        return Ok((plane, wires, true));
    }
    let profile = placed_curve(embedded_curve(entity)?, transform)?;
    let closed = profile.curve.is_closed();
    Ok((profile.plane, vec![sweep_profile_pieces(&profile.curve)?], closed))
}

enum HistorySweepPath {
    Planar { plane: Plane, curves: Vec<Curve>, start: [f64; 3] },
    Polyline3d { points: Vec<[f64; 3]>, closed: bool },
    Nurbs3(NurbsCurve3),
}

impl HistorySweepPath {
    fn borrowed(&self) -> brep::SweepPath<'_> {
        match self {
            Self::Planar { plane, curves, .. } => brep::SweepPath::Planar {
                plane: *plane,
                curves,
            },
            Self::Polyline3d { points, closed } => brep::SweepPath::Polyline3d {
                points,
                closed: *closed,
            },
            Self::Nurbs3(curve) => brep::SweepPath::Nurbs3(curve),
        }
    }

    fn start(&self) -> Option<[f64; 3]> {
        match self {
            Self::Planar { start, .. } => Some(*start),
            Self::Polyline3d { points, .. } => points.first().copied(),
            Self::Nurbs3(curve) => Some(curve.point_at(0.0)),
        }
    }

    fn end(&self) -> Option<[f64; 3]> {
        match self {
            Self::Planar { plane, curves, .. } => {
                let reversed = brep::reversed_path_curves(curves)?;
                Some(plane.point_at(reversed.first()?.point_at(0.0)))
            }
            Self::Polyline3d { points, closed } => if *closed { points.first().copied() } else { points.last().copied() },
            Self::Nurbs3(curve) => Some(curve.point_at(1.0)),
        }
    }

    /// The same path traversed from its other end.
    fn reversed(self) -> Option<Self> {
        Some(match self {
            Self::Planar { plane, curves, .. } => {
                let curves = brep::reversed_path_curves(&curves)?;
                let start = plane.point_at(curves.first()?.point_at(0.0));
                Self::Planar { plane, curves, start }
            }
            Self::Polyline3d { mut points, closed } => {
                points.reverse();
                Self::Polyline3d { points, closed }
            }
            Self::Nurbs3(curve) => Self::Nurbs3(curve.reversed()?),
        })
    }

    fn translated(mut self, shift: Vec3) -> Result<Self, HistoryRebuildError> {
        match &mut self {
            Self::Planar { plane, start, .. } => {
                plane.origin = (Vec3::from(plane.origin) + shift).to_array();
                *start = (Vec3::from(*start) + shift).to_array();
            }
            Self::Polyline3d { points, .. } => {
                for point in points {
                    *point = (Vec3::from(*point) + shift).to_array();
                }
            }
            Self::Nurbs3(curve) => {
                *curve = NurbsCurve3::new_strict(
                    curve.degree(),
                    curve
                        .control_points()
                        .iter()
                        .map(|point| (Vec3::from(*point) + shift).to_array())
                        .collect(),
                    curve.knots().to_vec(),
                    curve.weights().to_vec(),
                )
                .ok_or(HistoryRebuildError::InvalidParameters)?
                .with_periodicity(curve.periodicity());
            }
        }
        Ok(self)
    }
}

fn embedded_sweep_path(
    entity: &EmbeddedEntity,
    transform: [f64; 16],
) -> Result<HistorySweepPath, HistoryRebuildError> {
    // A polyline kept as a wire body: straight spans are a 3D polyline;
    // a planar chain with arcs is swept as a bulged polyline in its plane.
    if let Some(spans) = body_wire_spans(entity) {
        let place = placement(transform)?;
        if place.scale().is_none() {
            return Err(HistoryRebuildError::InvalidTransform);
        }
        if spans.iter().all(|span| matches!(span.curve, brep::Curve3::Line(_))) {
            let mut points = std::iter::once(spans[0].start)
                .chain(spans.iter().map(|span| span.end))
                .map(|point| place.point(point))
                .collect::<Vec<_>>();
            let closed = points.len() > 2
                && Vec3::from(points[0]).distance(Vec3::from(*points.last().unwrap()))
                    <= coplanarity_tolerance(&points).max(1e-9);
            if closed {
                points.pop();
            }
            return Ok(HistorySweepPath::Polyline3d { points, closed });
        }
    }
    if let EmbeddedEntity::Spline(value) = entity {
        let degree = value.degree.max(1) as usize;
        let fit_method = !value.fit_points.is_empty() && value.control_points.len() <= degree;
        let place = placement(transform)?;
        if place.scale().is_none() {
            return Err(HistoryRebuildError::InvalidTransform);
        }
        if degree == 1 && !fit_method && value.weights.is_empty() {
            let points = value
                .control_points
                .iter()
                .map(|point| place.point([point.x, point.y, point.z]))
                .collect::<Vec<_>>();
            if points.len() < 2 || points.iter().flatten().any(|value| !value.is_finite()) {
                return Err(HistoryRebuildError::InvalidParameters);
            }
            return Ok(HistorySweepPath::Polyline3d {
                points,
                closed: value.flags.closed || value.flags.periodic,
            });
        }
        if !fit_method {
            let controls = value
                .control_points
                .iter()
                .map(|point| place.point([point.x, point.y, point.z]))
                .collect::<Vec<_>>();
            let curve = NurbsCurve3::new(
                degree,
                controls,
                value.knots.clone(),
                (!value.weights.is_empty()).then(|| value.weights.clone()),
            )
            .ok_or(HistoryRebuildError::InvalidParameters)?;
            let curve = NurbsCurve3::new_strict(
                curve.degree(),
                curve.control_points().to_vec(),
                curve.knots().to_vec(),
                curve.weights().to_vec(),
            )
            .ok_or(HistoryRebuildError::InvalidParameters)?
            .with_periodicity(value.flags.periodic);
            return Ok(HistorySweepPath::Nurbs3(curve));
        }
        if value.flags.periodic {
            let points = value
                .fit_points
                .iter()
                .map(|point| place.point([point.x, point.y, point.z]))
                .collect::<Vec<_>>();
            let parameterization = match value.knot_parameterization {
                2 => Parameterization::Uniform,
                1 => Parameterization::Centripetal,
                _ => Parameterization::Chord,
            };
            return NurbsCurve3::interpolate_periodic(&points, parameterization)
                .map(HistorySweepPath::Nurbs3)
                .ok_or(HistoryRebuildError::InvalidParameters);
        }
        let mut points = value.fit_points.iter()
            .map(|point| [point.x, point.y, point.z]).collect::<Vec<_>>();
        if value.flags.closed && points.first() != points.last() && !points.is_empty() {
            points.push(points[0]);
        }
        let parameterization = match value.knot_parameterization {
            2 => Parameterization::Uniform,
            1 => Parameterization::Centripetal,
            _ => Parameterization::Chord,
        };
        let (controls, knots) = kernel::space::spline::interpolate_open(
            &points,
            Some([value.begin_tangent.x, value.begin_tangent.y, value.begin_tangent.z]),
            Some([value.end_tangent.x, value.end_tangent.y, value.end_tangent.z]),
            parameterization,
        ).ok_or(HistoryRebuildError::InvalidParameters)?;
        let weights = vec![1.0; controls.len()];
        // Interpolate in source space before applying placement: scaling a
        // placed fit must also preserve its endpoint derivative constraints.
        return NurbsCurve3::new_strict(3, controls.into_iter().map(|point| place.point(point)).collect(),
            knots, weights).map(HistorySweepPath::Nurbs3)
            .ok_or(HistoryRebuildError::InvalidParameters);
    }
    let path = placed_curve(embedded_curve(entity)?, transform)?;
    Ok(HistorySweepPath::Planar {
        plane: path.plane,
        start: path.point_at(0.0),
        curves: vec![path.curve],
    })
}

fn sweep_nurbs_length(curve: &NurbsCurve3) -> f64 {
    const NODES: [(f64, f64); 5] = [
        (-0.906_179_845_938_664, 0.236_926_885_056_189),
        (-0.538_469_310_105_683, 0.478_628_670_499_366),
        (0.0, 0.568_888_888_888_889),
        (0.538_469_310_105_683, 0.478_628_670_499_366),
        (0.906_179_845_938_664, 0.236_926_885_056_189),
    ];
    let (start, end) = curve.domain();
    curve.knots().windows(2).filter_map(|pair| {
        let from = pair[0].max(start);
        let to = pair[1].min(end);
        (to > from).then_some((from, to))
    }).map(|(from, to)| {
        let width = (to - from) / 8.0;
        (0..8).map(|panel| {
            let half = width * 0.5;
            let middle = from + width * (panel as f64 + 0.5);
            half * NODES.iter().map(|(node, weight)| {
                weight * Vec3::from(curve.tangent_at_knot(middle + half * node)).length()
            }).sum::<f64>()
        }).sum::<f64>()
    }).sum()
}

/// Transforms that place the embedded profile and path in world space.
///
/// With flag 295 both entities are stored already placed (the profile at the
/// path start, both in world coordinates). The record's matrices then
/// describe the source profile frame and the placed profile frame; applying
/// them to the stored geometry again would move it off the path.
fn sweep_entity_transforms(value: &SolidHistorySweep) -> ([f64; 16], [f64; 16]) {
    const IDENTITY: [f64; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    if value.flags_294_296[1] {
        (IDENTITY, IDENTITY)
    } else {
        (value.sweep_entity_transform, value.path_entity_transform)
    }
}

/// Length of the history path in world units, including its closing segment.
pub fn sweep_history_path_length(value: &SolidHistorySweep) -> Result<f64, HistoryRebuildError> {
    let path = embedded_sweep_path(
        value.path_entity.as_ref().ok_or(HistoryRebuildError::InvalidParameters)?,
        sweep_entity_transforms(value).1,
    )?;
    let length = match path {
        HistorySweepPath::Planar { plane, curves, .. } => {
            curves.iter().map(Curve::length).sum::<f64>() * Vec3::from(plane.x_axis).length()
        }
        HistorySweepPath::Polyline3d { points, closed } => {
            let mut length = points.windows(2).map(|pair| {
                (Vec3::from(pair[1]) - Vec3::from(pair[0])).length()
            }).sum::<f64>();
            if closed {
                length += (Vec3::from(points[0]) - Vec3::from(*points.last().unwrap())).length();
            }
            length
        }
        HistorySweepPath::Nurbs3(curve) => sweep_nurbs_length(&curve),
    };
    let scale = placement(value.base.transform)?.scale().ok_or(HistoryRebuildError::InvalidTransform)?;
    let length = length * scale;
    (length.is_finite() && length >= 0.0)
        .then_some(length)
        .ok_or(HistoryRebuildError::InvalidParameters)
}

struct SweepHistoryGeometry {
    plane: Plane,
    wires: Vec<Vec<Curve>>,
    path: HistorySweepPath,
    path_shift: Vec3,
    options: brep::SweepOptions,
}

fn sweep_history_geometry(
    value: &SolidHistorySweep,
    surface: bool,
) -> Result<SweepHistoryGeometry, HistoryRebuildError> {
    let (profile_transform, path_transform) = sweep_entity_transforms(value);
    let (plane, wires, closed) = sweep_profile_geometry(
        value.sweep_entity.as_ref().ok_or(HistoryRebuildError::InvalidParameters)?,
        profile_transform,
    )?;
    let mut path = embedded_sweep_path(
        value.path_entity.as_ref().ok_or(HistoryRebuildError::InvalidParameters)?,
        path_transform,
    )?;
    // Flag 295: the stored profile is already placed at the path start and
    // aligned (base point and alignment applied), so it is swept where it
    // stands, turned by the profile rotation about the start tangent.
    if value.flags_294_296[1] {
        // The placed profile stands at the path end the sweep starts from;
        // an open path is traversed from that end.
        // The placed frame is the record's path matrix; the geometry itself
        // is already in world coordinates (identity `path_transform`).
        let placed = Vec3::from(placement(value.path_entity_transform)?.origin);
        if let (Some(start), Some(end)) = (path.start(), path.end()) {
            if Vec3::from(end).distance(placed) + 1e-9 < Vec3::from(start).distance(placed) {
                path = path.reversed().ok_or(HistoryRebuildError::InvalidParameters)?;
            }
        }
        let start = brep::sweep_path_start(path.borrowed()).ok_or(HistoryRebuildError::InvalidParameters)?;
        let tangent = Vec3::from(brep::sweep_path_tangent(path.borrowed()).ok_or(HistoryRebuildError::InvalidParameters)?);
        let facing = Vec3::from(plane.normal().ok_or(HistoryRebuildError::InvalidParameters)?).dot(tangent);
        return Ok(SweepHistoryGeometry {
            plane,
            wires,
            path,
            path_shift: Vec3::ZERO,
            options: brep::SweepOptions {
                align: false,
                base_point: Some(start),
                rotation: if facing < 0.0 { -value.align_angle } else { value.align_angle },
                twist: value.twist_angle,
                scale: value.scale_factor,
                bank: value.bank,
                surface: surface || !closed,
                // The profile is stored placed; its drawn plane is the
                // record's source frame.
                profile_axes: Some([
                    [value.sweep_entity_transform[0], value.sweep_entity_transform[1], value.sweep_entity_transform[2]],
                    [value.sweep_entity_transform[4], value.sweep_entity_transform[5], value.sweep_entity_transform[6]],
                ]),
            },
        });
    }
    let explicit_alignment = value.has_align_start || value.align_option != 0;
    let mut path_shift = Vec3::ZERO;
    let reference_point = if explicit_alignment {
        [value.reference_point.x, value.reference_point.y, value.reference_point.z]
    } else {
        let pieces = wires.first().ok_or(HistoryRebuildError::InvalidParameters)?;
        let center = pieces.iter()
            .map(|piece| Vec3::from(plane.point_at(piece.point_at(0.0))))
            .fold(Vec3::ZERO, |sum, point| sum + point) / pieces.len() as f64;
        let start = Vec3::from(path.start().ok_or(HistoryRebuildError::InvalidParameters)?);
        path_shift = center - start;
        path = path.translated(path_shift)?;
        center.to_array()
    };
    Ok(SweepHistoryGeometry {
        plane,
        wires,
        path,
        path_shift,
        options: brep::SweepOptions {
            // Option 2 translates the profile to the path without turning it.
            align: explicit_alignment && value.align_option == 1,
            base_point: Some(reference_point),
            rotation: value.align_angle,
            twist: value.twist_angle,
            scale: value.scale_factor,
            bank: value.bank,
            surface: surface || !closed,
            profile_axes: None,
        },
    })
}

fn compose_placements(outer: Placement, inner: Placement) -> Placement {
    Placement {
        origin: outer.point(inner.origin),
        x_axis: outer.vector(inner.x_axis),
        y_axis: outer.vector(inner.y_axis),
        z_axis: outer.vector(inner.z_axis),
    }
}

/// World placements of the embedded profile and path used by the sweep.
/// Editing clients use their inverses so grips follow the displayed geometry.
pub fn sweep_history_placements(
    value: &SolidHistorySweep,
) -> Result<(Placement, Placement), HistoryRebuildError> {
    let geometry = sweep_history_geometry(value, false)?;
    let profile = brep::sweep_profile_placement(
        geometry.plane, &geometry.wires, geometry.path.borrowed(), geometry.options,
    ).ok_or(HistoryRebuildError::InvalidParameters)?;
    let base = placement(value.base.transform)?;
    let path_shift = Placement {
        origin: geometry.path_shift.to_array(),
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
    };
    let (profile_transform, path_transform) = sweep_entity_transforms(value);
    Ok((
        compose_placements(base, compose_placements(profile, placement(profile_transform)?)),
        compose_placements(base, compose_placements(path_shift, placement(path_transform)?)),
    ))
}

/// The base point of a profile swept along a path when none was picked:
/// the path start when the path starts in the profile's plane inside or on
/// the profile, else the profile's own anchor.
pub fn sweep_default_base(
    profile: &EmbeddedEntity,
    profile_transform: [f64; 16],
    path: &EmbeddedEntity,
) -> Result<[f64; 3], HistoryRebuildError> {
    let (plane, wires, _) = sweep_profile_geometry(profile, profile_transform)?;
    let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let path = embedded_sweep_path(path, identity)?;
    let start = brep::sweep_path_start(path.borrowed()).ok_or(HistoryRebuildError::InvalidParameters)?;
    let tangent = brep::sweep_path_tangent(path.borrowed()).ok_or(HistoryRebuildError::InvalidParameters)?;
    brep::sweep_profile_base_from(plane, &wires, start, tangent).ok_or(HistoryRebuildError::InvalidParameters)
}

/// The displayed reference point; new aligned sweeps reference the path start.
pub fn sweep_history_reference_point(value: &SolidHistorySweep) -> Result<[f64; 3], HistoryRebuildError> {
    let reference = if value.has_align_start || value.align_option != 0 {
        let path = embedded_sweep_path(
            value.path_entity.as_ref().ok_or(HistoryRebuildError::InvalidParameters)?,
            sweep_entity_transforms(value).1,
        )?;
        brep::sweep_path_start(path.borrowed()).ok_or(HistoryRebuildError::InvalidParameters)?
    } else {
        [value.reference_point.x, value.reference_point.y, value.reference_point.z]
    };
    Ok(placement(value.base.transform)?.point(reference))
}

/// Rebuilds a sweep, retaining whether its owning entity is a sheet or a solid.
///
/// New records with an explicit alignment start place their reference point on
/// the path. Older records retain their original profile placement, because
/// those writers translated the path to the profile rather than moving it.
pub fn rebuild_sweep_with_mode(
    value: &SolidHistorySweep,
    surface: bool,
) -> Result<Body, HistoryRebuildError> {
    // The reference application builds the same bisector-mitered corner for
    // every miter option (default, old, new, crimp, bend: 0..=4), on planar
    // and spatial polyline paths alike. Unknown values stay unsupported.
    if value.miter_option > 4 {
        return Err(HistoryRebuildError::Unsupported);
    }
    if !value.scale_factor.is_finite()
        || value.scale_factor <= 1e-9
        || !value.draft_angle.is_finite()
        || !value.twist_angle.is_finite()
        || !value.align_angle.is_finite()
    {
        return Err(HistoryRebuildError::InvalidParameters);
    }
    if value.draft_angle.abs() > 1e-12 {
        return if !surface && !value.has_align_start {
            legacy_sweep(value)
        } else {
            Err(HistoryRebuildError::Unsupported)
        };
    }
    if let Some(spatial) = rebuild_spatial_sweep(value) {
        return spatial;
    }
    let geometry = sweep_history_geometry(value, surface)?;
    if let Some(why) = brep::sweep_corner_refusal(geometry.path.borrowed(), geometry.options) {
        return Err(HistoryRebuildError::Refused(why));
    }
    finish(
        brep::sweep_path(
            geometry.plane,
            &geometry.wires,
            geometry.path.borrowed(),
            geometry.options,
        ),
        value.base.transform,
    )
}

/// Why a sweep record is refused: a twist or scale along a path with a
/// corner, or banking along a planar one.
pub fn sweep_history_refusal(value: &SolidHistorySweep) -> Option<brep::SweepRefusal> {
    let geometry = sweep_history_geometry(value, false).ok()?;
    brep::sweep_corner_refusal(geometry.path.borrowed(), geometry.options)
}

/// The vertices of a straight-sided wire-body profile that does not lie in
/// one plane, with whether it is closed.
pub fn sweep_spatial_profile(entity: &EmbeddedEntity, transform: [f64; 16]) -> Option<(Vec<[f64; 3]>, bool)> {
    let spans = body_wire_spans(entity)?;
    if spans.is_empty() || !spans.iter().all(|span| matches!(span.curve, brep::Curve3::Line(_))) {
        return None;
    }
    let place = placement(transform).ok()?;
    let mut points = spans.iter().map(|span| place.point(span.start)).collect::<Vec<_>>();
    let last = place.point(spans.last()?.end);
    let closed = Vec3::from(last).distance(Vec3::from(points[0])) <= 1e-9 * Vec3::from(points[0]).length().max(1.0);
    if !closed { points.push(last); }
    // Planar profiles take the ordinary route.
    let first = Vec3::from(points[0]);
    let normal = (0..points.len()).fold(Vec3::ZERO, |sum, index| {
        let a = Vec3::from(points[index]) - first;
        let b = Vec3::from(points[(index + 1) % points.len()]) - first;
        sum + a.cross(b)
    }).normalize()?;
    let size = points.iter().map(|p| Vec3::from(*p).distance(first)).fold(1.0_f64, f64::max);
    let planar = points.iter().all(|p| (Vec3::from(*p) - first).dot(normal).abs() <= size * 1e-9);
    (!planar).then_some((points, closed))
}

/// Whether the record's path has a corner.
pub fn sweep_history_path_has_corner(value: &SolidHistorySweep) -> Option<bool> {
    let (_, path_transform) = sweep_entity_transforms(value);
    let path = embedded_sweep_path(value.path_entity.as_ref()?, path_transform).ok()?;
    brep::sweep_path_has_corner(path.borrowed())
}

/// A sweep record whose profile is a spatial polyline: a swept surface.
fn rebuild_spatial_sweep(value: &SolidHistorySweep) -> Option<Result<Body, HistoryRebuildError>> {
    let (profile_transform, path_transform) = sweep_entity_transforms(value);
    let (points, closed) = sweep_spatial_profile(value.sweep_entity.as_ref()?, profile_transform)?;
    let path = match embedded_sweep_path(value.path_entity.as_ref()?, path_transform) {
        Ok(path) => path,
        Err(error) => return Some(Err(error)),
    };
    let options = brep::SweepOptions {
        align: value.align_option == 1,
        // Group 294: a base point the user picked.
        base_point: value.flags_294_296[0]
            .then_some([value.reference_point.x, value.reference_point.y, value.reference_point.z]),
        rotation: value.align_angle,
        twist: value.twist_angle,
        scale: value.scale_factor,
        bank: value.bank,
        surface: true,
        profile_axes: None,
    };
    Some(finish(brep::sweep_spatial_polyline(&points, closed, path.borrowed(), options), value.base.transform))
}

fn rebuild_sweep(value: &SolidHistorySweep) -> Result<Body, HistoryRebuildError> {
    rebuild_sweep_with_mode(value, false)
}

fn embedded_line_path_3d(entity: &EmbeddedEntity) -> Option<Vec<[f64; 3]>> {
    let EmbeddedEntity::Spline(value) = entity else {
        return None;
    };
    if value.degree != 1
        || value.flags.closed
        || value.control_points.len() < 2
        || !value.weights.is_empty()
    {
        return None;
    }
    let points = value
        .control_points
        .iter()
        .map(|point| [point.x, point.y, point.z])
        .collect::<Vec<_>>();
    points
        .windows(2)
        .all(|pair| (Vec3::from(pair[1]) - Vec3::from(pair[0])).length() > 1e-12)
        .then_some(points)
}

fn rebuild_extrusion(value: &SolidHistorySweep) -> Result<Body, HistoryRebuildError> {
    rebuild_extrusion_with_mode(value, false)
}

/// Rebuilds extrusion history without losing region holes or open sheet mode.
pub fn rebuild_extrusion_with_mode(value: &SolidHistorySweep, surface: bool) -> Result<Body, HistoryRebuildError> {
    if !value.scale_factor.is_finite()
        || (value.scale_factor - 1.0).abs() > 1e-9
        || !value.draft_angle.is_finite()
        || !value.twist_angle.is_finite()
        || value.twist_angle.abs() > 1e-9
        || !value.align_angle.is_finite()
        || value.align_angle.abs() > 1e-9
    {
        return Err(HistoryRebuildError::Unsupported);
    }
    // Flags 295/296: the profile and path are stored where they stand, and
    // the matrices only describe their frames (the reference writes an
    // extrusion so, with the profile frame at its anchor).
    let (profile_transform, path_transform) = sweep_entity_transforms(value);
    let (plane, wires, closed) = sweep_profile_geometry(
        value.sweep_entity.as_ref().ok_or(HistoryRebuildError::InvalidParameters)?,
        profile_transform,
    )?;
    if !surface && !closed { return Err(HistoryRebuildError::InvalidParameters); }
    let body = if let Some(path) = value.path_entity.as_ref() {
        if value.draft_angle.abs() > 1e-9 || surface || wires.len() != 1 {
            return Err(HistoryRebuildError::Unsupported);
        }
        let pieces = &wires[0];
        let mut path = placed_curve(embedded_curve(path)?, path_transform)?;
        let profile_center = pieces
            .iter()
            .map(|piece| Vec3::from(plane.point_at(piece.point_at(0.0))))
            .fold(Vec3::ZERO, |sum, point| sum + point)
            / pieces.len() as f64;
        let path_start = Vec3::from(path.plane.point_at(path.curve.point_at(0.0)));
        path.plane.origin =
            (Vec3::from(path.plane.origin) + profile_center - path_start).to_array();
        brep::sweep_along(plane, pieces, path.plane, &path_pieces(&path.curve)?)
    } else {
        let direction = [value.direction.x, value.direction.y, value.direction.z];
        #[cfg(feature = "offset")]
        {
            if surface {
                brep::extrude_surface_region_tapered(plane, &wires, direction, value.draft_angle)
            } else {
                brep::extrude_region_tapered(plane, &wires, direction, value.draft_angle)
            }
        }
        #[cfg(not(feature = "offset"))]
        {
            if value.draft_angle.abs() > 1e-9 {
                return Err(HistoryRebuildError::Unsupported);
            }
            if surface {
                brep::extrude_surface_region(plane, &wires, direction)
            } else {
                brep::extrude_region(plane, &wires, direction)
            }
        }
    };
    finish(body, value.base.transform)
}

fn rotate_vector(value: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (sine, cosine) = angle.sin_cos();
    value * cosine + axis.cross(value) * sine + axis * axis.dot(value) * (1.0 - cosine)
}

fn rebuild_revolve(value: &SolidHistoryRevolve) -> Result<Body, HistoryRebuildError> {
    if !value.revolve_angle.is_finite()
        || value.revolve_angle.abs() <= 1e-12
        || !value.start_angle.is_finite()
        || [
            value.draft_angle,
            value.twist_angle,
            value.field_44,
            value.field_45,
        ]
        .iter()
        .any(|parameter| !parameter.is_finite() || parameter.abs() > 1e-12)
        || !value.flag_290
        || value.close_to_axis
    {
        return Err(HistoryRebuildError::Unsupported);
    }
    let mut profile = embedded_curve(
        value
            .sweep_entity
            .as_ref()
            .ok_or(HistoryRebuildError::InvalidParameters)?,
    )?;
    let axis_origin = Vec3::from([
        value.axis_point.x,
        value.axis_point.y,
        value.axis_point.z,
    ]);
    let axis = Vec3::from([
        value.direction.x,
        value.direction.y,
        value.direction.z,
    ])
    .normalize()
    .ok_or(HistoryRebuildError::InvalidParameters)?;
    if !axis_origin.is_finite() || !axis.is_finite() {
        return Err(HistoryRebuildError::InvalidParameters);
    }
    if value.start_angle.abs() > 1e-12 {
        let origin = Vec3::from(profile.plane.origin);
        profile.plane.origin =
            (axis_origin + rotate_vector(origin - axis_origin, axis, value.start_angle))
                .to_array();
        profile.plane.x_axis =
            rotate_vector(Vec3::from(profile.plane.x_axis), axis, value.start_angle).to_array();
        profile.plane.y_axis =
            rotate_vector(Vec3::from(profile.plane.y_axis), axis, value.start_angle).to_array();
    }
    finish(
        brep::revolve(
            profile.plane,
            &profile_pieces(&profile.curve)?,
            axis_origin.to_array(),
            axis.to_array(),
            value.revolve_angle,
        ),
        value.base.transform,
    )
}

/// Decode a section or a joined chain without losing rational curves or region holes.
pub fn loft_section_geometry(entities: &[EmbeddedEntity]) -> Result<brep::LoftSection, String> {
    let identity = [1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0];
    if let [EmbeddedEntity::Point(point)] = entities {
        return Ok(brep::LoftSection::Point([point.location.x, point.location.y, point.location.z]));
    }
    let mut profiles = entities.iter().map(|entity| sweep_profile_geometry(entity, identity)
        .map_err(|error| format!("Invalid loft section: {error}")))
        .collect::<Result<Vec<_>, _>>()?;
    if profiles.is_empty() { return Err("Select a curve for each loft section.".into()); }
    if profiles.len() == 1 {
        let (plane, wires, closed) = profiles.remove(0);
        return Ok(brep::LoftSection::Profile { plane, wires, closed });
    }
    if profiles.iter().any(|(_, wires, closed)| *closed || wires.len() != 1) {
        return Err("Join only connected open edges in one section.".into());
    }
    // A single line's arbitrary supporting plane need not contain the other
    // selected edges. Derive the joined plane from the entire spatial chain.
    let mut spatial = Vec::new();
    for (plane, wires, _) in &profiles {
        for curve in &wires[0] {
            spatial.extend(brep::RationalCurve2::from_curve(curve)
                .ok_or("Unsupported joined section curve")?.lifted(plane).points);
        }
    }
    let origin = Vec3::from(*spatial.first().ok_or("Empty joined section")?);
    let tolerance = coplanarity_tolerance(&spatial);
    let axis = spatial.iter().map(|point| Vec3::from(*point)-origin)
        .find(|axis| axis.length() > tolerance).and_then(Vec3::normalize)
        .ok_or("Degenerate joined section")?;
    let normal = spatial.iter().map(|point| axis.cross(Vec3::from(*point)-origin))
        .find(|normal| normal.length() > tolerance).and_then(Vec3::normalize);
    let plane = match normal {
        Some(normal) => Plane::orthonormal(origin.to_array(), axis.to_array(), normal.to_array())
            .ok_or("Invalid joined section plane")?,
        None => Plane::from_axes(origin.to_array(), profiles[0].0.x_axis, profiles[0].0.y_axis),
    };
    let normal = Vec3::from(plane.normal().ok_or("Invalid section plane")?);
    let mut curves = Vec::new();
    for (source_plane, wires, _) in profiles {
        for curve in &wires[0] {
            let rational = brep::RationalCurve2::from_curve(curve)
                .ok_or("Unsupported joined section curve")?.lifted(&source_plane);
            let scale = rational.points.iter().map(|point| (Vec3::from(*point) - Vec3::from(plane.origin)).length())
                .fold(1.0_f64, f64::max);
            let mut points = Vec::new();
            for point in rational.points {
                if (Vec3::from(point) - Vec3::from(plane.origin)).dot(normal).abs() > scale * 1e-8 {
                    return Err("Joined section edges must be coplanar.".into());
                }
                points.push(plane.project(point).ok_or("Invalid section plane")?);
            }
            curves.push(NurbsCurve::new_strict(rational.degree, points, rational.knots, rational.weights)
                .ok_or("Invalid joined section curve")?);
        }
    }
    let distance = |a: [f64;2], b: [f64;2]| (a[0]-b[0]).hypot(a[1]-b[1]);
    let scale = curves.iter().map(|curve| curve.control_points().iter().map(|point| point[0].hypot(point[1]))
        .fold(1.0_f64, f64::max)).fold(1.0_f64, f64::max);
    let tolerance = scale * 1e-8;
    let mut chain = vec![curves.remove(0)];
    while !curves.is_empty() {
        let end = chain.last().unwrap().point_at(1.0);
        if let Some((at, reverse)) = curves.iter().enumerate().find_map(|(at, curve)| {
            if distance(end, curve.point_at(0.0)) <= tolerance { Some((at, false)) }
            else if distance(end, curve.point_at(1.0)) <= tolerance { Some((at, true)) }
            else { None }
        }) {
            let curve = curves.remove(at);
            chain.push(if reverse { curve.reversed() } else { curve });
            continue;
        }
        let start = chain.first().unwrap().point_at(0.0);
        if let Some((at, reverse)) = curves.iter().enumerate().find_map(|(at, curve)| {
            if distance(start, curve.point_at(1.0)) <= tolerance { Some((at, false)) }
            else if distance(start, curve.point_at(0.0)) <= tolerance { Some((at, true)) }
            else { None }
        }) {
            let curve = curves.remove(at);
            chain.insert(0, if reverse { curve.reversed() } else { curve });
        } else { return Err("Joined section edges must form one connected chain.".into()); }
    }
    let closed = distance(chain[0].point_at(0.0), chain.last().unwrap().point_at(1.0)) <= tolerance;
    Ok(brep::LoftSection::Profile { plane, wires: vec![chain.into_iter().map(Curve::Nurbs).collect()], closed })
}

/// Bounded, normalized three-dimensional guide/path curves.
pub fn loft_path_geometry(entity: &EmbeddedEntity) -> Result<Vec<brep::Curve3>, String> {
    let identity = [1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0];
    match embedded_sweep_path(entity, identity).map_err(|error| format!("Invalid loft guide or path: {error}"))? {
        HistorySweepPath::Planar { plane, curves, start } => {
            let pieces = curves.iter().map(sweep_profile_pieces)
                .collect::<Result<Vec<_>, _>>().map_err(|error| format!("Invalid loft path pieces: {error}"))?;
            let mut previous = Vec3::from(start);
            pieces.into_iter().flatten().map(|curve| {
            let mut rational = brep::RationalCurve2::from_curve(&curve)
                .ok_or("Unsupported loft guide or path")?.lifted(&plane);
            let initial = rational.curve().ok_or("Invalid loft guide or path")?;
            if previous.distance(Vec3::from(initial.point_at(1.0)))
                < previous.distance(Vec3::from(initial.point_at(0.0))) {
                rational = rational.reversed();
            }
            let oriented = rational.curve().ok_or("Invalid loft guide or path")?;
            previous = Vec3::from(oriented.point_at(1.0));
            Ok(brep::Curve3::Nurbs(oriented))
            }).collect()
        },
        HistorySweepPath::Polyline3d { points, closed } => {
            let mut points = points;
            if closed && points.first() != points.last() { points.push(points[0]); }
            Ok(points.windows(2).map(|pair| brep::Curve3::Line(brep::Line3 {
                origin: pair[0], direction: (Vec3::from(pair[1])-Vec3::from(pair[0])).to_array(),
            })).collect())
        }
        HistorySweepPath::Nurbs3(curve) => {
            let (start, end) = curve.domain();
            let knots = curve.knots().iter().map(|value| (value-start)/(end-start)).collect();
            Ok(vec![brep::Curve3::Nurbs(NurbsCurve3::new_strict(curve.degree(), curve.control_points().to_vec(),
                knots, curve.weights().to_vec()).ok_or("Invalid loft path spline")?)])
        }
    }
}

/// First creation, Properties changes and reload all use this same builder.
pub fn rebuild_loft_with_options(value: &SolidHistoryLoft) -> Result<Body, String> {
    let settings = value.parameters.clone().unwrap_or_else(|| document::objects::SolidHistoryLoftParameters {
        normals: 0, ..Default::default()
    });
    let counts = if settings.section_counts.is_empty() { vec![1; value.cross_sections.len()] }
        else { settings.section_counts.clone() };
    if counts.iter().any(|count| *count == 0) || counts.iter().try_fold(0usize, |sum, count| sum.checked_add(*count))
        != Some(value.cross_sections.len()) { return Err("Invalid loft section grouping.".into()); }
    let mut offset = 0;
    let mut sections = Vec::new();
    for count in counts {
        sections.push(loft_section_geometry(&value.cross_sections[offset..offset+count])?);
        offset += count;
    }
    let guides = value.guides.iter().map(loft_path_geometry).collect::<Result<Vec<_>, _>>()?;
    let path = settings.path_entity.as_ref().map(loft_path_geometry).transpose()?;
    let body = brep::loft_with_options(&sections, &guides, path.as_deref(), brep::LoftOptions {
        surface: settings.surface, normals: settings.normals,
        start_draft_angle: settings.start_draft_angle, end_draft_angle: settings.end_draft_angle,
        start_magnitude: settings.start_magnitude, end_magnitude: settings.end_magnitude,
        start_continuity: settings.start_continuity, end_continuity: settings.end_continuity,
        start_bulge: settings.start_bulge, end_bulge: settings.end_bulge,
        closed: settings.closed, periodic: settings.periodic, align_direction: settings.align_direction,
    }).map_err(|error| error.to_string())?;
    brep::transform(&body, &placement(value.base.transform).map_err(|error| error.to_string())?)
        .ok_or_else(|| "Invalid loft placement.".into())
}

fn rebuild_loft(value: &SolidHistoryLoft) -> Result<Body, HistoryRebuildError> {
    rebuild_loft_with_options(value).map_err(|_| HistoryRebuildError::InvalidParameters)
}

pub fn rebuild_body(
    operation: &SolidHistoryOperation,
) -> Result<Body, HistoryRebuildError> {
    match operation {
        // Box and wedge history frames sit at the bounding-box centre.
        SolidHistoryOperation::Box(value) => finish(
            brep::make::cuboid(
                [-value.length * 0.5, -value.width * 0.5, -value.height * 0.5],
                [value.length, value.width, value.height],
            ),
            value.base.transform,
        ),
        SolidHistoryOperation::Wedge(value) => finish(
            brep::make::wedge(
                [-value.length * 0.5, -value.width * 0.5, -value.height * 0.5],
                value.length,
                value.width,
                value.height,
            ),
            value.base.transform,
        ),
        SolidHistoryOperation::Sphere(value) => finish(
            brep::make::sphere([0.0; 3], value.radius),
            value.base.transform,
        ),
        SolidHistoryOperation::Cylinder(value) => {
            if [value.major_radius, value.minor_radius, value.x_radius]
                .iter()
                .any(|radius| !radius.is_finite() || *radius <= 0.0)
            {
                return Err(HistoryRebuildError::InvalidParameters);
            }
            finish(
                brep::make::elliptical_cylinder(
                    [0.0; 3],
                    value.major_radius,
                    value.minor_radius,
                    value.height,
                ),
                value.base.transform,
            )
        }
        SolidHistoryOperation::Cone(value) => {
            finish(
                brep::make::frustum(
                    [0.0; 3],
                    value.base_x_radius,
                    value.base_y_radius,
                    value.top_radius,
                    value.height,
                ),
                value.base.transform,
            )
        }
        SolidHistoryOperation::Pyramid(value) => {
            if value.sides < 3 {
                return Err(HistoryRebuildError::InvalidParameters);
            }
            finish(
                brep::make::pyramid_frustum(
                    [0.0; 3],
                    value.radius,
                    value.top_radius,
                    value.height,
                    value.sides as usize,
                ),
                value.base.transform,
            )
        }
        SolidHistoryOperation::Torus(value) => finish(
            brep::make::torus(
                [0.0; 3],
                value.major_radius,
                value.minor_radius,
            ),
            value.base.transform,
        ),
        SolidHistoryOperation::Brep(value) => {
            let document = value
                .acis_data
                .parse()
                .ok_or(HistoryRebuildError::InvalidBrep)?;
            let (bodies, _) = super::lift(&document);
            finish(bodies.into_iter().next(), value.base.transform)
                .map_err(|error| match error {
                    HistoryRebuildError::InvalidParameters => {
                        HistoryRebuildError::InvalidBrep
                    }
                    other => other,
                })
        }
        SolidHistoryOperation::Sweep(value) => rebuild_sweep(value),
        SolidHistoryOperation::Extrusion(value) => rebuild_extrusion(value),
        SolidHistoryOperation::Loft(value) => rebuild_loft(value),
        SolidHistoryOperation::Revolve(value) => rebuild_revolve(value),
        _ => Err(HistoryRebuildError::Unsupported),
    }
}

/// Rebuilds a creation operation followed by its recorded edge operations.
///
/// Edge and face references are zero-based ordinals into the current body's
/// deterministic key order. Each operation is atomic: its result replaces the
/// current body only after every selected edge succeeds.
pub fn rebuild_history(
    operations: &[SolidHistoryOperation],
) -> Result<Body, HistoryRebuildError> {
    let (first, following) = operations
        .split_first()
        .ok_or(HistoryRebuildError::InvalidParameters)?;
    let mut body = rebuild_body(first)?;
    for operation in following {
        body = rebuild_step(operation, std::slice::from_ref(&body))?;
    }
    Ok(body)
}

/// Rebuild a history tree: each step over what its operands rebuild to, so a
/// boolean step combines both solids it joined.
pub fn rebuild_history_tree(tree: &SolidHistoryTree) -> Result<Body, HistoryRebuildError> {
    if tree.operands.is_empty() {
        return rebuild_body(&tree.operation);
    }
    let operands = tree
        .operands
        .iter()
        .map(rebuild_history_tree)
        .collect::<Result<Vec<_>, _>>()?;
    rebuild_step(&tree.operation, &operands)
}

/// One step over the bodies of its operands, placed by the step's own
/// transform.
fn rebuild_step(
    operation: &SolidHistoryOperation,
    operands: &[Body],
) -> Result<Body, HistoryRebuildError> {
    let (rebuilt, transform) = match (operation, operands) {
        (SolidHistoryOperation::Fillet(value), [body]) => {
            let current_edges = body.edge_keys().collect::<Vec<_>>();
            let edges = selected_edges(&current_edges, &value.edges)?;
            let radius = *value
                .radii
                .first()
                .ok_or(HistoryRebuildError::InvalidParameters)?;
            (brep::fillet_edges(body, &edges, radius)?, value.base.transform)
        }
        (SolidHistoryOperation::Chamfer(value), [body]) => {
            let current_edges = body.edge_keys().collect::<Vec<_>>();
            let faces = body.face_keys().collect::<Vec<_>>();
            let base_face = usize::try_from(value.base_face)
                .ok()
                .and_then(|ordinal| faces.get(ordinal).copied())
                .ok_or(HistoryRebuildError::InvalidParameters)?;
            (
                brep::chamfer_edges(
                    body,
                    &selected_edges(&current_edges, &value.edges)?,
                    base_face,
                    value.base_distance,
                    value.other_distance,
                )?,
                value.base.transform,
            )
        }
        (SolidHistoryOperation::Boolean(value), [first, second]) => {
            let how = match value.operation {
                SolidHistoryBoolean::UNION => brep::Operation::Union,
                SolidHistoryBoolean::INTERSECT => brep::Operation::Intersection,
                SolidHistoryBoolean::SUBTRACT => brep::Operation::Difference,
                _ => return Err(HistoryRebuildError::Unsupported),
            };
            let tolerance = brep::operation_tolerance(&[first, second]);
            let combined = brep::combine(first.clone(), second.clone(), how, tolerance)
                .map_err(|_| HistoryRebuildError::Boolean)?;
            (combined, value.base.transform)
        }
        _ => return Err(HistoryRebuildError::Unsupported),
    };
    brep::transform(&rebuilt, &placement(transform)?).ok_or(HistoryRebuildError::InvalidTransform)
}

fn selected_edges<K: Copy>(edges: &[K], ordinals: &[i32]) -> Result<Vec<K>, HistoryRebuildError> {
    ordinals
        .iter()
        .map(|ordinal| {
            usize::try_from(*ordinal)
                .ok()
                .and_then(|ordinal| edges.get(ordinal).copied())
                .ok_or(HistoryRebuildError::InvalidParameters)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::objects::{SolidHistoryBox, SolidHistoryNodeBase};

    fn cube(id: i32, size: [f64; 3]) -> SolidHistoryTree {
        SolidHistoryTree {
            operation: SolidHistoryOperation::Box(SolidHistoryBox {
                base: SolidHistoryNodeBase::new(id),
                length: size[0],
                width: size[1],
                height: size[2],
                ..SolidHistoryBox::default()
            }),
            operands: Vec::new(),
        }
    }

    fn volume(body: &Body) -> f64 {
        let mesh = brep::mesh::body(body, kernel::tessellation::DEFAULT_ANGLE, 1e-9);
        mesh.mass_properties().expect("a closed result").0
    }

    #[test]
    fn a_boolean_step_rebuilds_from_both_operands() {
        let mut tree = SolidHistoryTree {
            operation: SolidHistoryOperation::Boolean(SolidHistoryBoolean {
                base: SolidHistoryNodeBase::new(3),
                operation: SolidHistoryBoolean::SUBTRACT,
                first_operand: 1,
                second_operand: 2,
                ..SolidHistoryBoolean::default()
            }),
            operands: vec![cube(1, [4.0, 4.0, 4.0]), cube(2, [2.0, 2.0, 8.0])],
        };
        let body = rebuild_history_tree(&tree).expect("a box with a square hole");
        assert!((volume(&body) - 48.0).abs() < 1e-6, "{}", volume(&body));

        // Editing the tool reshapes the composite.
        let SolidHistoryOperation::Box(tool) = &mut tree.find_mut(2).unwrap().operation else {
            unreachable!();
        };
        tool.length = 1.0;
        let body = rebuild_history_tree(&tree).expect("a box with a narrower hole");
        assert!((volume(&body) - 56.0).abs() < 1e-6, "{}", volume(&body));
    }
}
