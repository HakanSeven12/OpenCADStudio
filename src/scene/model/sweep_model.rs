// Exact profile sweeps stored as kernel B-reps and ACIS.

use cadkernel::brep::{self, Body};
use cadkernel::geom2d::{offset_polyline, Arc, Curve, EllipseArc, Line, Polyline};
use cadkernel::space::{PlanarCurve, Plane, Vec3};
use acadrust::EntityType;

use crate::entities::curve::entity_curve;

/// A drawn profile, as the kernel wants it: the plane it lies in and the
/// chain of pieces closing a loop in that plane.
pub struct Profile {
    pub plane: Plane,
    pub pieces: Vec<Curve>,
}

/// The closed profile an entity describes, or `None` when it does not
/// describe one.
///
/// A single closed curve — a circle, an ellipse, a closed polyline — is what
/// a sweep needs. The kernel wants it as a chain of at least three pieces,
/// which a polyline already is and which a circle is not, so a round profile
/// is handed over as the arcs it is made of rather than as one curve with
/// nowhere for the sweep to start.
pub fn profile_of(entity: &EntityType) -> Option<Profile> {
    let planar: PlanarCurve = entity_curve(entity)?;
    if !planar.curve.is_closed() {
        return None;
    }
    let pieces = match &planar.curve {
        Curve::Polyline(_) => planar.curve.segments(),
        Curve::Circle(circle) => quarters(circle.centre, circle.radius),
        Curve::Arc(arc) => split_arc(*arc),
        Curve::Ellipse(arc) => split_ellipse(*arc),
        Curve::Nurbs(curve) => (0..4)
            .map(|part| {
                curve
                    .trimmed(part as f64 / 4.0, (part + 1) as f64 / 4.0)
                    .map(Curve::Nurbs)
            })
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    (pieces.len() >= 3).then_some(Profile {
        plane: planar.plane,
        pieces,
    })
}

/// A circle as four arcs.
fn quarters(centre: [f64; 2], radius: f64) -> Vec<Curve> {
    use std::f64::consts::FRAC_PI_2;
    (0..4)
        .map(|quarter| {
            let start = FRAC_PI_2 * quarter as f64;
            Curve::Arc(Arc {
                centre,
                radius,
                start_angle: start,
                end_angle: start + FRAC_PI_2,
            })
        })
        .collect()
}

fn split_arc(arc: Arc) -> Vec<Curve> {
    let start = arc.start_angle;
    let step = arc.sweep() / 4.0;
    (0..4)
        .map(|part| {
            Curve::Arc(Arc {
                start_angle: start + step * part as f64,
                end_angle: start + step * (part + 1) as f64,
                ..arc
            })
        })
        .collect()
}

fn split_ellipse(arc: EllipseArc) -> Vec<Curve> {
    let start = arc.start_parameter;
    let step = arc.sweep() / 4.0;
    (0..4)
        .map(|part| {
            Curve::Ellipse(EllipseArc {
                ellipse: arc.ellipse,
                start_parameter: start + step * part as f64,
                end_parameter: start + step * (part + 1) as f64,
            })
        })
        .collect()
}

/// EXTRUDE: drag the profile `height` along its own plane's normal.
///
/// `None` for a profile that does not close, encloses nothing, or holds a
/// piece with no analytic side wall.
pub fn extruded(entity: &EntityType, height: f64) -> Option<Body> {
    let profile = profile_of(entity)?;
    let normal = profile.plane.normal()?;
    brep::extrude(
        profile.plane,
        &profile.pieces,
        [normal[0] * height, normal[1] * height, normal[2] * height],
    )
}

/// Signed distance of a drag along a profile's normal.
pub fn projected_drag(entity: &EntityType, from: glam::DVec3, to: glam::DVec3) -> Option<f64> {
    let normal = entity_curve(entity)?.plane.normal()?;
    let distance = (to - from).dot(glam::DVec3::from_array(normal));
    (distance.is_finite() && distance.abs() > 1e-6).then_some(distance)
}

/// REVOLVE: turn the profile about the axis from `from` to `to` by `angle`
/// radians.
///
/// The axis has to lie in the profile's plane — a profile and an axis that do
/// not share one sweep into surfaces with no analytic form, and the kernel
/// refuses rather than approximating them.
pub fn revolved(
    entity: &EntityType,
    from: [f64; 3],
    to: [f64; 3],
    angle: f64,
) -> Option<Body> {
    let profile = profile_of(entity)?;
    brep::revolve(
        profile.plane,
        &profile.pieces,
        from,
        [to[0] - from[0], to[1] - from[1], to[2] - from[2]],
        angle,
    )
}

/// SWEEP as an exact B-rep along straight and circular path pieces.
pub fn swept(profile: &EntityType, path: &EntityType) -> Option<Body> {
    let profile = profile_of(profile)?;
    let path = entity_curve(path)?;
    let pieces = match &path.curve {
        Curve::Line(line) => vec![Curve::Line(*line)],
        Curve::Arc(arc) => vec![Curve::Arc(*arc)],
        Curve::Polyline(_) => path.curve.segments(),
        _ => return None,
    };
    brep::sweep_along(profile.plane, &profile.pieces, path.plane, &pieces)
}

/// LOFT as an exact B-rep through compatible closed sections.
pub fn lofted(profiles: &[EntityType]) -> Option<Body> {
    if let Some(body) = circular_loft(profiles) {
        return Some(body);
    }
    let sections = profiles
        .iter()
        .map(|entity| {
            let profile = profile_of(entity)?;
            Some((profile.plane, profile.pieces))
        })
        .collect::<Option<Vec<_>>>()?;
    brep::loft(&sections)
}

/// Builds an exact wall solid around a polyline centreline.
pub fn polysolid(entity: &EntityType, width: f64, height: f64) -> Option<Body> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height.abs() <= 1e-12 {
        return None;
    }
    let planar = entity_curve(entity)?;
    let Curve::Polyline(source) = planar.curve else {
        return None;
    };
    let (left_pick, right_pick) = offset_picks(&source, width)?;
    let mut left = offset_polyline(&source, width * 0.5, left_pick);
    let mut right = offset_polyline(&source, width * 0.5, right_pick);
    if left.len() != 1 || right.len() != 1 {
        return None;
    }
    let left = left.remove(0);
    let right = right.remove(0);
    let normal = Vec3::from(planar.plane.normal()?);
    let direction = (normal * height).to_array();

    if source.closed {
        if !left.closed || !right.closed {
            return None;
        }
        let mut profiles = [left, right]
            .into_iter()
            .map(|polyline| {
                let area = Curve::Polyline(polyline.clone()).enclosed_area().abs();
                (area, Curve::Polyline(polyline).segments())
            })
            .collect::<Vec<_>>();
        profiles.sort_by(|first, second| second.0.total_cmp(&first.0));
        if profiles[0].0 <= profiles[1].0 || profiles[1].0 <= 1e-12 {
            return None;
        }
        return brep::extrude_region(
            planar.plane,
            &[profiles.remove(0).1, profiles.remove(0).1],
            direction,
        );
    }

    if left.closed || right.closed {
        return None;
    }
    let left_start = left.vertices.first()?.position;
    let left_end = left.vertices.last()?.position;
    let right_start = right.vertices.first()?.position;
    let right_end = right.vertices.last()?.position;
    let mut outline = Curve::Polyline(left).segments();
    outline.push(Curve::Line(Line {
        start: left_end,
        end: right_end,
    }));
    let mut back = Curve::Polyline(right).segments();
    back.reverse();
    outline.extend(back);
    outline.push(Curve::Line(Line {
        start: right_start,
        end: left_start,
    }));
    brep::extrude(planar.plane, &outline, direction)
}

fn offset_picks(polyline: &Polyline, width: f64) -> Option<([f64; 2], [f64; 2])> {
    let start = polyline.vertices.first()?.position;
    let next = polyline.vertices.get(1)?.position;
    let along = if let Some(arc) = polyline.segment_arc(0) {
        let near = arc.sample(1e-6);
        [near[0] - start[0], near[1] - start[1]]
    } else {
        [next[0] - start[0], next[1] - start[1]]
    };
    let length = along[0].hypot(along[1]);
    if length <= 1e-12 {
        return None;
    }
    let side = [-along[1] / length * width, along[0] / length * width];
    Some((
        [start[0] + side[0], start[1] + side[1]],
        [start[0] - side[0], start[1] - side[1]],
    ))
}

fn circular_loft(profiles: &[EntityType]) -> Option<Body> {
    if profiles.len() < 2 {
        return None;
    }
    let sections = profiles
        .iter()
        .map(|entity| {
            let planar = entity_curve(entity)?;
            let Curve::Circle(circle) = planar.curve else {
                return None;
            };
            Some((
                Vec3::from(planar.plane.point_at(circle.centre)),
                circle.radius,
                Vec3::from(planar.plane.normal()?),
                Vec3::from(planar.plane.x_axis),
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let first = sections.first()?;
    let direction = (sections.last()?.0 - first.0).normalize()?;
    let scale = sections
        .iter()
        .map(|(centre, radius, _, _)| centre.length().max(*radius))
        .fold(1.0_f64, f64::max);
    let tolerance = scale * 1e-9;
    let mut heights = Vec::with_capacity(sections.len());
    for (centre, radius, normal, _) in &sections {
        if !radius.is_finite()
            || *radius <= tolerance
            || normal.dot(direction).abs() < 1.0 - 1e-9
        {
            return None;
        }
        let offset = *centre - first.0;
        if (offset - direction * offset.dot(direction)).length() > tolerance {
            return None;
        }
        heights.push(offset.dot(direction));
    }
    if heights
        .windows(2)
        .any(|pair| pair[1] <= pair[0] + tolerance)
    {
        return None;
    }
    let radial_seed = first.3 - direction * first.3.dot(direction);
    let radial = radial_seed.normalize()?;
    let profile_plane = Plane::from_axes(first.0.to_array(), radial.to_array(), direction.to_array());
    let mut points = Vec::with_capacity(sections.len() + 2);
    points.push([0.0, 0.0]);
    points.extend(
        sections
            .iter()
            .zip(&heights)
            .map(|((_, radius, _, _), height)| [*radius, *height]),
    );
    points.push([0.0, *heights.last()?]);
    let profile = (0..points.len())
        .map(|index| {
            Curve::Line(Line {
                start: points[index],
                end: points[(index + 1) % points.len()],
            })
        })
        .collect::<Vec<_>>();
    brep::revolve(
        profile_plane,
        &profile,
        first.0.to_array(),
        direction.to_array(),
        std::f64::consts::TAU,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{Circle, LwPolyline, LwVertex};
    use acadrust::types::{Vector2, Vector3};

    fn square(size: f64) -> EntityType {
        let mut polyline = LwPolyline::new();
        for corner in [[0.0, 0.0], [size, 0.0], [size, size], [0.0, size]] {
            polyline.add_vertex(LwVertex::new(Vector2::new(corner[0], corner[1])));
        }
        polyline.is_closed = true;
        EntityType::LwPolyline(polyline)
    }

    fn volume(body: &Body) -> f64 {
        crate::scene::model::solid_model::volume(body)
    }

    #[test]
    fn a_closed_polyline_extrudes_into_a_prism() {
        let solid = extruded(&square(10.0), 4.0).expect("a prism");
        assert!((volume(&solid) - 400.0).abs() < 1e-6, "{}", volume(&solid));
    }

    #[test]
    fn a_circle_extrudes_into_a_cylinder() {
        // Cut into quarter arcs on the way, so the wall is four cylinder
        // patches rather than a run of flat chords — the volume says which.
        let mut circle = Circle::new();
        circle.center = Vector3::new(0.0, 0.0, 0.0);
        circle.radius = 5.0;
        let solid = extruded(&EntityType::Circle(circle), 3.0).expect("a cylinder");
        let expected = std::f64::consts::PI * 25.0 * 3.0;
        let got = volume(&solid);
        assert!(got > 0.98 * expected, "{got} vs {expected}");
        assert!(got <= expected * 1.000_001, "{got} vs {expected}");
    }

    #[test]
    fn a_square_revolved_about_its_own_edge_is_a_tube() {
        // The axis runs up the square's left side, so what it sweeps is a
        // solid cylinder of radius ten and height ten.
        let solid = revolved(
            &square(10.0),
            [0.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            std::f64::consts::TAU,
        );
        // The profile is in the XY plane and the axis lies in it, so this is
        // a revolution about the y axis: radius ten, height ten.
        let solid = solid.expect("a cylinder");
        let expected = std::f64::consts::PI * 100.0 * 10.0;
        let got = volume(&solid);
        assert!(got > 0.98 * expected, "{got} vs {expected}");
    }

    #[test]
    fn an_open_profile_is_refused() {
        let mut polyline = LwPolyline::new();
        for corner in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]] {
            polyline.add_vertex(LwVertex::new(Vector2::new(corner[0], corner[1])));
        }
        polyline.is_closed = false;
        assert!(extruded(&EntityType::LwPolyline(polyline), 4.0).is_none());
    }

    #[test]
    fn an_axis_off_the_profiles_plane_is_refused() {
        assert!(revolved(
            &square(10.0),
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 1.0],
            std::f64::consts::TAU
        )
        .is_none());
    }
}
