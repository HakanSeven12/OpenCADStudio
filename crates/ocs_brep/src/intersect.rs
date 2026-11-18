//! What two surfaces share.
//!
//! The question a boolean is built on: to cut one solid with another, the
//! curves where their faces meet have to be found first, and everything
//! afterwards — splitting faces, choosing which pieces to keep, stitching the
//! result — depends on those curves being right.
//!
//! # Exactly, or not at all
//!
//! Where a closed form exists it is used and the answer is exact. Two spheres
//! share a circle; a plane and a cylinder share an ellipse; two planes share
//! a line. Marching a numeric intersection across those would put every seam
//! a tolerance off its true place, and the error compounds through each
//! boolean.
//!
//! Where no closed form is implemented the answer is [`Meeting::Unknown`],
//! not an empty one. The distinction matters more than it looks: a caller
//! told "they do not meet" will happily union two solids that overlap and
//! produce a shape with a hole in it, while one told "I cannot say" can
//! refuse. Silence about ignorance is the expensive failure here.
//!
//! # Where there is no closed form
//!
//! The general quartic cases — two cylinders at an angle, a sphere against a
//! cone — and every spline pair say `Unknown` here. Bounded pieces of any two
//! surfaces meet through [`ssi`](super::ssi), which finds their crossing and
//! tangent curves, touch points and branch points.

use super::geometry::{Circle3, Cone, Curve3, Cylinder, Ellipse3, Line3, Sphere, Surface, Torus};
use ocs_geom::space::{Plane, Vec3};

/// What two surfaces have in common.
#[derive(Debug, Clone, PartialEq)]
pub enum Meeting {
    /// Nothing. They are apart.
    None,
    /// These curves, exactly.
    Curves(Vec<Curve3>),
    /// These isolated points — a tangency rather than a crossing.
    Points(Vec<[f64; 3]>),
    /// The same surface, over whatever region both cover.
    Coincident,
    /// No closed form for this pair is implemented, and none was
    /// approximated. Not the same as [`None`](Meeting::None): a caller must
    /// refuse rather than assume they are apart.
    Unknown,
}

/// Where two surfaces meet.
///
/// `tolerance` decides when two directions count as parallel and when a
/// near-tangency is treated as a touch.
pub fn surfaces(a: &Surface, b: &Surface, tolerance: f64) -> Meeting {
    // One surface twice, of whatever kind — a spline patch included, which
    // has no closed form to compare by.
    if a == b {
        return Meeting::Coincident;
    }
    match (a, b) {
        (Surface::Plane(one), Surface::Plane(other)) => planes(one, other, tolerance),
        (Surface::Plane(plane), Surface::Sphere(sphere))
        | (Surface::Sphere(sphere), Surface::Plane(plane)) => {
            plane_sphere(plane, sphere, tolerance)
        }
        (Surface::Sphere(one), Surface::Sphere(other)) => spheres(one, other, tolerance),
        (Surface::Plane(plane), Surface::Cylinder(cylinder))
        | (Surface::Cylinder(cylinder), Surface::Plane(plane)) => {
            plane_cylinder(plane, cylinder, tolerance)
        }
        (Surface::Plane(plane), Surface::Cone(cone))
        | (Surface::Cone(cone), Surface::Plane(plane)) => plane_cone(plane, cone, tolerance),
        (Surface::Cylinder(one), Surface::Cylinder(other)) => {
            cylinders(one, other, tolerance)
        }
        (Surface::Sphere(sphere), Surface::Cylinder(cylinder))
        | (Surface::Cylinder(cylinder), Surface::Sphere(sphere)) => {
            sphere_cylinder(sphere, cylinder, tolerance)
        }
        (Surface::Torus(torus), Surface::Cylinder(cylinder))
        | (Surface::Cylinder(cylinder), Surface::Torus(torus)) => {
            torus_cylinder(torus, cylinder, tolerance)
        }
        (Surface::Plane(plane), Surface::Torus(torus))
        | (Surface::Torus(torus), Surface::Plane(plane)) => plane_torus(plane, torus, tolerance)
            .or_else(|| coaxial_revolutions(a, b, tolerance))
            .or_else(|| apart(a, b, tolerance))
            .unwrap_or(Meeting::Unknown),
        // One torus twice — a fillet both solids of an assembly carry.
        (Surface::Torus(one), Surface::Torus(other)) if same_torus(one, other, tolerance) => {
            Meeting::Coincident
        }
        (Surface::Cone(cone), Surface::Cylinder(cylinder))
        | (Surface::Cylinder(cylinder), Surface::Cone(cone)) => coaxial_conics(
            &cone.base, cone.radius, cone.half_angle.tan(),
            &cylinder.base, cylinder.radius, 0.0, tolerance,
        ),
        (Surface::Cone(one), Surface::Cone(other)) => coaxial_conics(
            &one.base, one.radius, one.half_angle.tan(),
            &other.base, other.radius, other.half_angle.tan(), tolerance,
        ),
        _ => coaxial_revolutions(a, b, tolerance)
            .or_else(|| apart(a, b, tolerance))
            .unwrap_or(Meeting::Unknown),
    }
}

/// A surface of revolution's meridian in the half-plane of distance from an
/// axis and height along it, mirrored across the axis so a cone's two nappes
/// and a torus's two sides are one set of lines and circles.
enum Profile {
    Line { point: [f64; 2], direction: [f64; 2] },
    Circle { centre: [f64; 2], radius: f64 },
}

/// `surface`'s meridian about the line through `origin` along `axis`, when it
/// is a surface of revolution round that very line.
fn revolution_profile(
    surface: &Surface,
    origin: Vec3,
    axis: Vec3,
    tolerance: f64,
) -> Option<Vec<Profile>> {
    let height_of = |point: [f64; 3]| (Vec3::from(point) - origin).dot(axis);
    let on_axis = |point: [f64; 3]| {
        let offset = Vec3::from(point) - origin;
        (offset - axis * offset.dot(axis)).length() <= tolerance
    };
    let aligned = |frame: &Plane| -> Option<f64> {
        let own = Vec3::from(frame.normal()?);
        let sense = own.dot(axis);
        (own.cross(axis).length() <= 1e-9 && on_axis(frame.origin)).then_some(sense.signum())
    };
    Some(match surface {
        Surface::Plane(plane) => {
            let normal = Vec3::from(plane.normal()?);
            if normal.cross(axis).length() > 1e-9 {
                return None;
            }
            let height = height_of(plane.origin);
            vec![Profile::Line { point: [0.0, height], direction: [1.0, 0.0] }]
        }
        Surface::Cylinder(cylinder) => {
            aligned(&cylinder.base)?;
            let radius = cylinder.radius;
            vec![
                Profile::Line { point: [radius, 0.0], direction: [0.0, 1.0] },
                Profile::Line { point: [-radius, 0.0], direction: [0.0, 1.0] },
            ]
        }
        Surface::Cone(cone) => {
            let sense = aligned(&cone.base)?;
            let (height, slope) = (height_of(cone.base.origin), cone.half_angle.tan() * sense);
            vec![
                Profile::Line { point: [cone.radius, height], direction: [-slope, 1.0] },
                Profile::Line { point: [-cone.radius, height], direction: [slope, 1.0] },
            ]
        }
        Surface::Sphere(sphere) => {
            if !on_axis(sphere.frame.origin) {
                return None;
            }
            vec![Profile::Circle {
                centre: [0.0, height_of(sphere.frame.origin)],
                radius: sphere.radius,
            }]
        }
        Surface::Torus(torus) => {
            aligned(&torus.frame)?;
            let (major, minor) = (torus.major_radius.abs(), torus.minor_radius.abs());
            let height = height_of(torus.frame.origin);
            vec![
                Profile::Circle { centre: [major, height], radius: minor },
                Profile::Circle { centre: [-major, height], radius: minor },
            ]
        }
        Surface::Nurbs(_) | Surface::Offset(_) => return None,
    })
}

/// Two surfaces of revolution round one axis meet in circles round it: where
/// their meridians cross, each crossing swept round. A torus against a plane
/// square to its axis, a coaxial torus, a sphere or a cone on its axis
/// (#1563's knurled hubs and bearings).
/// Whether two tori are one: the same centre, axis and radii.
fn same_torus(one: &super::Torus, other: &super::Torus, tolerance: f64) -> bool {
    let (Some(first), Some(second)) = (one.frame.normal(), other.frame.normal()) else {
        return false;
    };
    Vec3::from(one.frame.origin).distance(Vec3::from(other.frame.origin)) <= tolerance
        && Vec3::from(first).is_parallel_to(Vec3::from(second), tolerance)
        && (one.major_radius - other.major_radius).abs() <= tolerance
        && (one.minor_radius - other.minor_radius).abs() <= tolerance
}

fn coaxial_revolutions(a: &Surface, b: &Surface, tolerance: f64) -> Option<Meeting> {
    let axis_of = |surface: &Surface| -> Option<(Vec3, Vec3)> {
        let frame = match surface {
            Surface::Cylinder(cylinder) => &cylinder.base,
            Surface::Cone(cone) => &cone.base,
            Surface::Torus(torus) => &torus.frame,
            _ => return None,
        };
        Some((Vec3::from(frame.origin), Vec3::from(frame.normal()?)))
    };
    let (origin, axis) = axis_of(a).or_else(|| axis_of(b))?;
    let one = revolution_profile(a, origin, axis, tolerance)?;
    let other = revolution_profile(b, origin, axis, tolerance)?;
    let mut crossings: Vec<[f64; 2]> = Vec::new();
    for first in &one {
        for second in &other {
            for point in profile_crossings(first, second, tolerance)? {
                let point = [point[0].abs(), point[1]];
                if !crossings.iter().any(|seen| {
                    (seen[0] - point[0]).abs() <= tolerance && (seen[1] - point[1]).abs() <= tolerance
                }) {
                    crossings.push(point);
                }
            }
        }
    }
    if crossings.is_empty() {
        return Some(Meeting::None);
    }
    let (mut curves, mut points) = (Vec::new(), Vec::new());
    for [radius, height] in crossings {
        match circle_on(origin + axis * height, axis, radius, tolerance) {
            Meeting::Curves(mut circle) => curves.append(&mut circle),
            Meeting::Points(mut pole) => points.append(&mut pole),
            _ => return Some(Meeting::Unknown),
        }
    }
    Some(if curves.is_empty() { Meeting::Points(points) } else { Meeting::Curves(curves) })
}

/// Where two meridians cross. `None` where they coincide over a stretch,
/// which is a shared surface rather than a crossing.
fn profile_crossings(one: &Profile, other: &Profile, tolerance: f64) -> Option<Vec<[f64; 2]>> {
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    Some(match (one, other) {
        (
            Profile::Line { point, direction },
            Profile::Line { point: other_point, direction: other_direction },
        ) => {
            let denominator = cross(*direction, *other_direction);
            let gap = [other_point[0] - point[0], other_point[1] - point[1]];
            if denominator.abs() <= 1e-12 {
                if cross(gap, *direction).abs() <= tolerance {
                    return None;
                }
                Vec::new()
            } else {
                let t = cross(gap, *other_direction) / denominator;
                vec![[point[0] + direction[0] * t, point[1] + direction[1] * t]]
            }
        }
        (Profile::Line { point, direction }, Profile::Circle { centre, radius })
        | (Profile::Circle { centre, radius }, Profile::Line { point, direction }) => {
            let length = direction[0].hypot(direction[1]);
            let unit = [direction[0] / length, direction[1] / length];
            let offset = [point[0] - centre[0], point[1] - centre[1]];
            let along = offset[0] * unit[0] + offset[1] * unit[1];
            let foot = [point[0] - unit[0] * along, point[1] - unit[1] * along];
            let distance = (foot[0] - centre[0]).hypot(foot[1] - centre[1]);
            if distance > radius + tolerance {
                Vec::new()
            } else {
                let half = (radius * radius - distance * distance).max(0.0).sqrt();
                if half <= tolerance {
                    vec![foot]
                } else {
                    vec![
                        [foot[0] - unit[0] * half, foot[1] - unit[1] * half],
                        [foot[0] + unit[0] * half, foot[1] + unit[1] * half],
                    ]
                }
            }
        }
        (
            Profile::Circle { centre, radius },
            Profile::Circle { centre: other_centre, radius: other_radius },
        ) => {
            let span = [other_centre[0] - centre[0], other_centre[1] - centre[1]];
            let distance = span[0].hypot(span[1]);
            if distance <= tolerance {
                if (radius - other_radius).abs() <= tolerance {
                    return None;
                }
                Vec::new()
            } else if distance > radius + other_radius + tolerance
                || distance < (radius - other_radius).abs() - tolerance
            {
                Vec::new()
            } else {
                let along = (distance * distance + radius * radius - other_radius * other_radius)
                    / (2.0 * distance);
                let half = (radius * radius - along * along).max(0.0).sqrt();
                let unit = [span[0] / distance, span[1] / distance];
                let foot = [centre[0] + unit[0] * along, centre[1] + unit[1] * along];
                if half <= tolerance {
                    vec![foot]
                } else {
                    vec![
                        [foot[0] - unit[1] * half, foot[1] + unit[0] * half],
                        [foot[0] + unit[1] * half, foot[1] - unit[0] * half],
                    ]
                }
            }
        }
    })
}

/// Two surfaces too far apart to meet, judged by the balls a sphere or a
/// torus fits in: bearings and hubs well clear of a shaft or of each other.
fn apart(a: &Surface, b: &Surface, tolerance: f64) -> Option<Meeting> {
    let ball = |surface: &Surface| -> Option<(Vec3, f64)> {
        match surface {
            Surface::Sphere(sphere) => Some((Vec3::from(sphere.frame.origin), sphere.radius)),
            Surface::Torus(torus) => Some((
                Vec3::from(torus.frame.origin),
                torus.major_radius.abs() + torus.minor_radius.abs(),
            )),
            _ => None,
        }
    };
    let clear = |bounded: (Vec3, f64), other: &Surface| -> bool {
        let (centre, reach) = bounded;
        match other {
            Surface::Plane(plane) => plane
                .distance_to(centre.to_array())
                .is_some_and(|distance| distance.abs() > reach + tolerance),
            Surface::Cylinder(cylinder) => cylinder.base.normal().is_some_and(|axis| {
                let axis = Vec3::from(axis);
                let offset = centre - Vec3::from(cylinder.base.origin);
                (offset - axis * offset.dot(axis)).length() > cylinder.radius + reach + tolerance
            }),
            _ => ball(other).is_some_and(|(other_centre, other_reach)| {
                centre.distance(other_centre) > reach + other_reach + tolerance
            }),
        }
    };
    match (ball(a), ball(b)) {
        (Some(bounded), _) if clear(bounded, b) => Some(Meeting::None),
        (_, Some(bounded)) if clear(bounded, a) => Some(Meeting::None),
        _ => None,
    }
}

/// A sphere centred on a cylinder's axis meets it in one or two exact
/// circles round that axis. The sphere's own frame plays no part: any axis
/// through its centre is one of its axes. Off the axis the section is a
/// quartic, which remains unknown until the marching path can represent
/// every branch safely.
fn sphere_cylinder(sphere: &Sphere, cylinder: &Cylinder, tolerance: f64) -> Meeting {
    let Some(axis) = cylinder.base.normal() else {
        return Meeting::Unknown;
    };
    let axis = Vec3::from(axis);
    let offset = Vec3::from(sphere.frame.origin) - Vec3::from(cylinder.base.origin);
    let along = offset.dot(axis);
    let across = (offset - axis * along).length();
    if across > tolerance {
        // A sphere wholly outside the cylinder or wholly inside it shares
        // nothing with it: ball bearings round a shaft (#1563).
        if across - sphere.radius > cylinder.radius + tolerance
            || across + sphere.radius < cylinder.radius - tolerance
        {
            return Meeting::None;
        }
        return Meeting::Unknown;
    }
    if cylinder.radius - sphere.radius > tolerance {
        return Meeting::None;
    }
    let squared = sphere.radius * sphere.radius - cylinder.radius * cylinder.radius;
    if squared < -tolerance * tolerance {
        return Meeting::None;
    }
    let rise = squared.max(0.0).sqrt();
    let heights = if rise <= tolerance {
        vec![along]
    } else {
        vec![along - rise, along + rise]
    };
    let mut curves = Vec::with_capacity(heights.len());
    for height in heights {
        let centre = Vec3::from(cylinder.base.origin) + axis * height;
        let Meeting::Curves(mut circle) = circle_on(centre, axis, cylinder.radius, tolerance)
        else {
            return Meeting::Unknown;
        };
        curves.append(&mut circle);
    }
    Meeting::Curves(curves)
}

/// A cylinder round a torus's own axis meets it where the tube reaches the
/// cylinder's radius: two circles, one where it only touches, or none — a
/// shaft through a knurled hub whose tube stands clear of it (#1563). Off
/// that axis the section is a quartic; only a cylinder clear of the whole
/// torus is answered.
/// A plane through a torus's axis cuts it in the tube's own section on
/// either side of the axis: two circles of the minor radius. `None` for any
/// other plane.
fn plane_torus(plane: &Plane, torus: &Torus, tolerance: f64) -> Option<Meeting> {
    let normal = Vec3::from(plane.normal()?).normalize()?;
    let axis = Vec3::from(torus.frame.normal()?).normalize()?;
    let centre = Vec3::from(torus.frame.origin);
    if normal.dot(axis).abs() > 1e-9
        || (centre - Vec3::from(plane.origin)).dot(normal).abs() > tolerance
    {
        return None;
    }
    let across = axis.cross(normal).normalize()?;
    let (major, minor) = (torus.major_radius.abs(), torus.minor_radius.abs());
    Some(Meeting::Curves(
        [1.0, -1.0]
            .map(|side| {
                Curve3::Circle(Circle3 {
                    plane: Plane {
                        origin: (centre + across * (major * side)).to_array(),
                        x_axis: across.to_array(),
                        y_axis: axis.to_array(),
                    },
                    radius: minor,
                })
            })
            .to_vec(),
    ))
}

fn torus_cylinder(torus: &Torus, cylinder: &Cylinder, tolerance: f64) -> Meeting {
    let (Some(axis), Some(torus_axis)) = (cylinder.base.normal(), torus.frame.normal()) else {
        return Meeting::Unknown;
    };
    let axis = Vec3::from(axis);
    let offset = Vec3::from(torus.frame.origin) - Vec3::from(cylinder.base.origin);
    let along = offset.dot(axis);
    let across = (offset - axis * along).length();
    let (major, minor) = (torus.major_radius.abs(), torus.minor_radius.abs());
    if across - (major + minor) > cylinder.radius + tolerance {
        return Meeting::None;
    }
    if axis.cross(Vec3::from(torus_axis)).length() > 1e-9 || across > tolerance {
        return Meeting::Unknown;
    }
    let gap = cylinder.radius - major;
    if gap.abs() > minor + tolerance {
        return Meeting::None;
    }
    let rise = (minor * minor - gap * gap).max(0.0).sqrt();
    let heights = if rise <= tolerance {
        vec![along]
    } else {
        vec![along - rise, along + rise]
    };
    let mut curves = Vec::with_capacity(heights.len());
    for height in heights {
        let centre = Vec3::from(cylinder.base.origin) + axis * height;
        let Meeting::Curves(mut circle) = circle_on(centre, axis, cylinder.radius, tolerance)
        else {
            return Meeting::Unknown;
        };
        curves.append(&mut circle);
    }
    Meeting::Curves(curves)
}

/// Coaxial cones and cylinders share exact circles, even when the two
/// parameter axes point in opposite directions. Both cone nappes are kept;
/// the faces' trims decide which circle belongs to the bounded solid.
fn coaxial_conics(one: &Plane, radius_one: f64, slope_one: f64, other: &Plane, radius_other: f64, slope_other: f64, tolerance: f64) -> Meeting {
    let (Some(axis), Some(other_axis)) = (one.normal(), other.normal()) else { return Meeting::Unknown; };
    let axis = Vec3::from(axis);
    let other_axis = Vec3::from(other_axis);
    if axis.cross(other_axis).length() > 1e-9 { return Meeting::Unknown; }
    let delta = Vec3::from(other.origin) - Vec3::from(one.origin);
    let shift = delta.dot(axis);
    if (delta - axis * shift).length() > tolerance { return Meeting::Unknown; }
    let slope_other = slope_other * axis.dot(other_axis);
    let other_at_origin = radius_other + slope_other * shift;
    let mut curves = Vec::new();
    for sign in [1.0, -1.0] {
        let denominator = slope_one - sign * slope_other;
        let numerator = radius_one - sign * other_at_origin;
        if denominator.abs() <= 1e-12 {
            if numerator.abs() <= tolerance { return Meeting::Coincident; }
            continue;
        }
        let height = numerator / denominator;
        let radius = (radius_one - slope_one * height).abs();
        if radius <= tolerance { continue; }
        let mut plane = *one;
        plane.origin = (Vec3::from(one.origin) + axis * height).to_array();
        if curves.iter().any(|curve: &Curve3| matches!(curve, Curve3::Circle(circle) if Vec3::from(circle.plane.origin).distance(Vec3::from(plane.origin)) <= tolerance)) { continue; }
        curves.push(Curve3::Circle(Circle3 { plane, radius }));
    }
    if curves.is_empty() { Meeting::None } else { Meeting::Curves(curves) }
}

/// Two planes: a line, nothing, or the same plane.
fn planes(one: &Plane, other: &Plane, tolerance: f64) -> Meeting {
    let (Some(first), Some(second)) = (one.normal(), other.normal()) else {
        return Meeting::Unknown;
    };
    let (first, second) = (Vec3::from(first), Vec3::from(second));
    let along = first.cross(second);
    if along.length() <= tolerance {
        // Parallel. The same plane if either origin lies on the other.
        return if other.contains(one.origin, tolerance) {
            Meeting::Coincident
        } else {
            Meeting::None
        };
    }
    // A point on the line, as the combination of the two normals that
    // satisfies both plane equations. Solved rather than found by dropping an
    // axis, which fails whenever the line runs along the axis dropped.
    let one_offset = first.dot(Vec3::from(one.origin));
    let other_offset = second.dot(Vec3::from(other.origin));
    let cosine = first.dot(second);
    let determinant = 1.0 - cosine * cosine;
    let scale_first = (one_offset - other_offset * cosine) / determinant;
    let scale_second = (other_offset - one_offset * cosine) / determinant;
    let origin = first * scale_first + second * scale_second;
    Meeting::Curves(vec![Curve3::Line(Line3 {
        origin: origin.to_array(),
        direction: along
            .normalize()
            .map_or(along, |unit| unit)
            .to_array(),
    })])
}

/// A plane and a sphere: a circle, a point of tangency, or nothing.
fn plane_sphere(plane: &Plane, sphere: &Sphere, tolerance: f64) -> Meeting {
    let Some(offset) = plane.distance_to(sphere.frame.origin) else {
        return Meeting::Unknown;
    };
    let Some(normal) = plane.normal() else {
        return Meeting::Unknown;
    };
    let centre = Vec3::from(sphere.frame.origin) - Vec3::from(normal) * offset;
    let gap = offset.abs() - sphere.radius;
    if gap > tolerance {
        return Meeting::None;
    }
    if gap.abs() <= tolerance {
        return Meeting::Points(vec![centre.to_array()]);
    }
    let radius = (sphere.radius * sphere.radius - offset * offset).max(0.0).sqrt();
    circle_on(centre, Vec3::from(normal), radius, tolerance)
}

/// Two spheres: a circle, a point, nothing, or the same sphere.
fn spheres(one: &Sphere, other: &Sphere, tolerance: f64) -> Meeting {
    let from = Vec3::from(one.frame.origin);
    let to = Vec3::from(other.frame.origin);
    let apart = to - from;
    let distance = apart.length();
    if distance <= tolerance {
        return if (one.radius - other.radius).abs() <= tolerance {
            Meeting::Coincident
        } else {
            // Concentric and different: one is wholly inside the other.
            Meeting::None
        };
    }
    let outer = one.radius + other.radius;
    let inner = (one.radius - other.radius).abs();
    if distance - outer > tolerance || inner - distance > tolerance {
        return Meeting::None;
    }
    let Some(axis) = apart.normalize() else {
        return Meeting::Unknown;
    };
    // How far along the line of centres the shared plane sits.
    let along = (distance * distance + one.radius * one.radius - other.radius * other.radius)
        / (2.0 * distance);
    let centre = from + axis * along;
    let squared = one.radius * one.radius - along * along;
    if squared <= tolerance * tolerance {
        return Meeting::Points(vec![centre.to_array()]);
    }
    circle_on(centre, axis, squared.sqrt(), tolerance)
}

/// A plane and a cylinder: a circle, an ellipse, one or two lines, or
/// nothing.
fn plane_cylinder(plane: &Plane, cylinder: &Cylinder, tolerance: f64) -> Meeting {
    let (Some(normal), Some(axis)) = (plane.normal(), cylinder.base.normal()) else {
        return Meeting::Unknown;
    };
    let (normal, axis) = (Vec3::from(normal), Vec3::from(axis));
    let cosine = normal.dot(axis);

    if cosine.abs() <= tolerance {
        // The plane runs along the axis: it cuts two generators, grazes one,
        // or misses.
        let Some(offset) = plane.distance_to(cylinder.base.origin) else {
            return Meeting::Unknown;
        };
        let gap = offset.abs() - cylinder.radius;
        if gap > tolerance {
            return Meeting::None;
        }
        // From the axis point, step onto the plane and then along it, by
        // however much the chord allows.
        let on_plane = Vec3::from(cylinder.base.origin) - normal * offset;
        if gap.abs() <= tolerance {
            return Meeting::Curves(vec![Curve3::Line(Line3 {
                origin: on_plane.to_array(),
                direction: axis.to_array(),
            })]);
        }
        let half = (cylinder.radius * cylinder.radius - offset * offset)
            .max(0.0)
            .sqrt();
        let Some(across) = axis.cross(normal).normalize() else {
            return Meeting::Unknown;
        };
        return Meeting::Curves(
            [half, -half]
                .into_iter()
                .map(|side| {
                    Curve3::Line(Line3 {
                        origin: (on_plane + across * side).to_array(),
                        direction: axis.to_array(),
                    })
                })
                .collect(),
        );
    }

    // The plane crosses the axis, so the section is closed. Where it crosses
    // is the section's centre.
    let Some(centre) = axis_meets_plane(cylinder.base.origin, axis, plane) else {
        return Meeting::Unknown;
    };
    if (cosine.abs() - 1.0).abs() <= tolerance {
        return circle_on(centre, axis, cylinder.radius, tolerance);
    }
    // Minor axis across the tilt, where the section is as narrow as the
    // cylinder; major along it, stretched by how far the plane leans.
    let Some(minor) = axis.cross(normal).normalize() else {
        return Meeting::Unknown;
    };
    let Some(major) = normal.cross(minor).normalize() else {
        return Meeting::Unknown;
    };
    let Some(frame) = Plane::orthonormal(centre.to_array(), major.to_array(), normal.to_array())
    else {
        return Meeting::Unknown;
    };
    Meeting::Curves(vec![Curve3::Ellipse(Ellipse3 {
        plane: frame,
        major_radius: cylinder.radius / cosine.abs(),
        minor_radius: cylinder.radius,
    })])
}

/// A plane and a cone.
///
/// Handled exactly where the section is a circle — the plane square to the
/// axis — or an ellipse, the plane cutting every generator of one nappe.
/// Parabolas and hyperbolas have no representation in [`Curve3`] yet, and
/// say [`Meeting::Unknown`].
fn plane_cone(plane: &Plane, cone: &Cone, tolerance: f64) -> Meeting {
    let (Some(normal), Some(axis)) = (plane.normal(), cone.base.normal()) else {
        return Meeting::Unknown;
    };
    let (normal, axis) = (Vec3::from(normal), Vec3::from(axis));
    let cosine = normal.dot(axis);

    if (cosine.abs() - 1.0).abs() <= tolerance {
        // Square to the axis: a circle whose radius is the cone's at that
        // height.
        let Some(centre) = axis_meets_plane(cone.base.origin, axis, plane) else {
            return Meeting::Unknown;
        };
        let height = (centre - Vec3::from(cone.base.origin)).dot(axis);
        let radius = cone.radius - height * cone.half_angle.tan();
        if radius.abs() <= tolerance {
            // The plane passes through the apex.
            return Meeting::Points(vec![centre.to_array()]);
        }
        if radius < 0.0 {
            // Past the apex, on the mirrored nappe. A cone record covers both
            // in ACIS, so this is a real circle rather than an error.
            return circle_on(centre, axis, -radius, tolerance);
        }
        return circle_on(centre, axis, radius, tolerance);
    }
    slanted_cone_ellipse(plane, cone, normal, axis, tolerance).unwrap_or(Meeting::Unknown)
}

/// The ellipse a slanted plane cuts from a cone, or `None` for a parabola or
/// hyperbola. Points `q` from the apex lie on the cone where
/// `|q|² = (1 + tan²α)(q·axis)²`; written in the plane's own coordinates
/// that is a conic, an ellipse when its quadratic part is definite.
fn slanted_cone_ellipse(
    plane: &Plane,
    cone: &Cone,
    normal: Vec3,
    axis: Vec3,
    tolerance: f64,
) -> Option<Meeting> {
    let slope = cone.half_angle.tan();
    if !slope.is_finite() || slope.abs() <= f64::EPSILON {
        return None;
    }
    let apex = Vec3::from(cone.base.origin) + axis * (cone.radius / slope);
    let normal = normal.normalize()?;
    let first = Vec3::from(plane.x_axis).normalize()?;
    let first = (first - normal * first.dot(normal)).normalize()?;
    let second = normal.cross(first);
    let k = 1.0 + slope * slope;
    let offset = Vec3::from(plane.origin) - apex;
    let (a1, a2, c0) = (first.dot(axis), second.dot(axis), offset.dot(axis));
    let quadratic = [[1.0 - k * a1 * a1, -k * a1 * a2], [-k * a1 * a2, 1.0 - k * a2 * a2]];
    let linear = [offset.dot(first) - k * c0 * a1, offset.dot(second) - k * c0 * a2];
    let constant = offset.dot(offset) - k * c0 * c0;
    let det = quadratic[0][0] * quadratic[1][1] - quadratic[0][1] * quadratic[1][0];
    if det <= 1e-12 {
        return None;
    }
    // Centre where the gradient vanishes, and the value the conic takes there.
    let centre = [
        -(linear[0] * quadratic[1][1] - linear[1] * quadratic[0][1]) / det,
        -(quadratic[0][0] * linear[1] - quadratic[1][0] * linear[0]) / det,
    ];
    let level = constant + linear[0] * centre[0] + linear[1] * centre[1];
    let trace = quadratic[0][0] + quadratic[1][1];
    let spread = ((quadratic[0][0] - quadratic[1][1]).powi(2)
        + 4.0 * quadratic[0][1] * quadratic[0][1])
        .sqrt();
    let (large, small) = (0.5 * (trace + spread), 0.5 * (trace - spread));
    let point = Vec3::from(plane.origin) + first * centre[0] + second * centre[1];
    let (minor, major) = ((-level / large).sqrt(), (-level / small).sqrt());
    if !(minor.is_finite() && major.is_finite()) || minor <= tolerance {
        // Through the apex: the section closes to it.
        return (level.abs() <= tolerance * tolerance.max(1.0)).then(|| {
            Meeting::Points(vec![point.to_array()])
        });
    }
    // The major axis runs along the eigenvector of the smaller eigenvalue.
    let along = if quadratic[0][1].abs() > f64::EPSILON {
        [quadratic[0][1], small - quadratic[0][0]]
    } else if quadratic[0][0] <= quadratic[1][1] {
        [1.0, 0.0]
    } else {
        [0.0, 1.0]
    };
    let direction = (first * along[0] + second * along[1]).normalize()?;
    let frame = Plane::orthonormal(point.to_array(), direction.to_array(), normal.to_array())?;
    Some(Meeting::Curves(vec![Curve3::Ellipse(Ellipse3 {
        plane: frame,
        major_radius: major,
        minor_radius: minor,
    })]))
}

/// Two cylinders. Coaxial and parallel-axis cases only; crossing axes give a
/// quartic that has no closed form here.
fn cylinders(one: &Cylinder, other: &Cylinder, tolerance: f64) -> Meeting {
    let (Some(first), Some(second)) = (one.base.normal(), other.base.normal()) else {
        return Meeting::Unknown;
    };
    let (first, second) = (Vec3::from(first), Vec3::from(second));
    if first.cross(second).length() > tolerance {
        return Meeting::Unknown;
    }
    // Parallel axes. How far apart they are decides everything.
    let between = Vec3::from(other.base.origin) - Vec3::from(one.base.origin);
    let across = between - first * between.dot(first);
    let distance = across.length();
    if distance <= tolerance {
        return if (one.radius - other.radius).abs() <= tolerance {
            Meeting::Coincident
        } else {
            Meeting::None
        };
    }
    let outer = one.radius + other.radius;
    let inner = (one.radius - other.radius).abs();
    if distance - outer > tolerance || inner - distance > tolerance {
        return Meeting::None;
    }
    let Some(towards) = across.normalize() else {
        return Meeting::Unknown;
    };
    // The generators they share sit where the two circles cross, swept along
    // the common axis direction.
    let along = (distance * distance + one.radius * one.radius - other.radius * other.radius)
        / (2.0 * distance);
    let midpoint = Vec3::from(one.base.origin) + towards * along;
    let squared = one.radius * one.radius - along * along;
    if squared <= tolerance * tolerance {
        return Meeting::Curves(vec![Curve3::Line(Line3 {
            origin: midpoint.to_array(),
            direction: first.to_array(),
        })]);
    }
    let Some(sideways) = first.cross(towards).normalize() else {
        return Meeting::Unknown;
    };
    let half = squared.sqrt();
    Meeting::Curves(
        [half, -half]
            .into_iter()
            .map(|side| {
                Curve3::Line(Line3 {
                    origin: (midpoint + sideways * side).to_array(),
                    direction: first.to_array(),
                })
            })
            .collect(),
    )
}

/// A circle of `radius` about `centre`, lying square to `normal`.
fn circle_on(centre: Vec3, normal: Vec3, radius: f64, tolerance: f64) -> Meeting {
    if radius <= tolerance {
        return Meeting::Points(vec![centre.to_array()]);
    }
    // Any direction in the plane will do for where the angle starts; the
    // circle is the same set whichever is picked.
    let seed = if normal.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let Some(frame) = Plane::orthonormal(centre.to_array(), seed.to_array(), normal.to_array())
    else {
        return Meeting::Unknown;
    };
    Meeting::Curves(vec![Curve3::Circle(Circle3 {
        plane: frame,
        radius,
    })])
}

/// Where a line through `origin` along `axis` crosses `plane`.
fn axis_meets_plane(origin: [f64; 3], axis: Vec3, plane: &Plane) -> Option<Vec3> {
    let normal = Vec3::from(plane.normal()?);
    let slope = axis.dot(normal);
    if slope == 0.0 {
        return None;
    }
    let offset = plane.distance_to(origin)?;
    Some(Vec3::from(origin) - axis * (offset / slope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_4, TAU};

    const TOL: f64 = 1e-9;

    fn plane_at(origin: [f64; 3], normal: [f64; 3]) -> Plane {
        let seed = if normal[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        Plane::orthonormal(origin, seed, normal).unwrap()
    }

    /// Every point of every curve reported, checked against both surfaces.
    /// The property that matters: an intersection curve is what the two share,
    /// so it must lie on each.
    fn assert_on_both(a: &Surface, b: &Surface, meeting: &Meeting) {
        let Meeting::Curves(curves) = meeting else {
            panic!("expected curves, got {meeting:?}");
        };
        assert!(!curves.is_empty());
        for curve in curves {
            for i in 0..=12 {
                // Lines are parameterised by length, the closed kinds by
                // angle; both are sampled over a range wide enough to show a
                // curve leaving its surface.
                let t = match curve {
                    Curve3::Line(_) => -5.0 + 10.0 * i as f64 / 12.0,
                    _ => TAU * i as f64 / 12.0,
                };
                let point = curve.point_at(t);
                assert!(
                    a.contains(point, 1e-6),
                    "{point:?} is {} off the first surface",
                    a.distance_to(point)
                );
                assert!(
                    b.contains(point, 1e-6),
                    "{point:?} is {} off the second surface",
                    b.distance_to(point)
                );
            }
        }
    }

    #[test]
    fn two_planes_share_a_line() {
        let ground = Surface::Plane(plane_at([0.0; 3], [0.0, 0.0, 1.0]));
        let wall = Surface::Plane(plane_at([0.0; 3], [1.0, 0.0, 0.0]));
        let meeting = surfaces(&ground, &wall, TOL);
        assert_on_both(&ground, &wall, &meeting);
    }

    #[test]
    fn two_planes_meeting_along_an_axis_still_find_the_line() {
        // The case dropping a coordinate to solve gets wrong: the shared line
        // runs along the axis that would be dropped.
        let one = Surface::Plane(plane_at([0.0, 0.0, 3.0], [0.0, 1.0, 1.0]));
        let other = Surface::Plane(plane_at([0.0, 0.0, 3.0], [0.0, 1.0, -1.0]));
        let meeting = surfaces(&one, &other, TOL);
        assert_on_both(&one, &other, &meeting);
    }

    #[test]
    fn parallel_planes_are_apart_or_the_same() {
        let low = Surface::Plane(plane_at([0.0; 3], [0.0, 0.0, 1.0]));
        let high = Surface::Plane(plane_at([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]));
        assert_eq!(surfaces(&low, &high, TOL), Meeting::None);
        let same = Surface::Plane(plane_at([9.0, 9.0, 0.0], [0.0, 0.0, 1.0]));
        assert_eq!(surfaces(&low, &same, TOL), Meeting::Coincident);
    }

    #[test]
    fn a_plane_cuts_a_sphere_in_a_circle() {
        let sphere = Surface::Sphere(Sphere {
            frame: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 5.0,
        });
        let plane = Surface::Plane(plane_at([0.0, 0.0, 3.0], [0.0, 0.0, 1.0]));
        let meeting = surfaces(&sphere, &plane, TOL);
        assert_on_both(&sphere, &plane, &meeting);
        let Meeting::Curves(curves) = &meeting else {
            unreachable!()
        };
        let Curve3::Circle(circle) = &curves[0] else {
            panic!("expected a circle");
        };
        // 3-4-5.
        assert!((circle.radius - 4.0).abs() < 1e-9, "{}", circle.radius);
    }

    #[test]
    fn a_plane_that_grazes_a_sphere_touches_at_a_point() {
        let sphere = Surface::Sphere(Sphere {
            frame: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 5.0,
        });
        let plane = Surface::Plane(plane_at([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]));
        match surfaces(&sphere, &plane, 1e-6) {
            Meeting::Points(points) => {
                assert_eq!(points.len(), 1);
                assert!((points[0][2] - 5.0).abs() < 1e-9);
            }
            other => panic!("expected a touch, got {other:?}"),
        }
        let clear = Surface::Plane(plane_at([0.0, 0.0, 6.0], [0.0, 0.0, 1.0]));
        assert_eq!(surfaces(&sphere, &clear, TOL), Meeting::None);
    }

    #[test]
    fn two_spheres_share_a_circle() {
        let one = Surface::Sphere(Sphere {
            frame: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 5.0,
        });
        let other = Surface::Sphere(Sphere {
            frame: plane_at([6.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            radius: 5.0,
        });
        let meeting = surfaces(&one, &other, TOL);
        assert_on_both(&one, &other, &meeting);
        let Meeting::Curves(curves) = &meeting else {
            unreachable!()
        };
        let Curve3::Circle(circle) = &curves[0] else {
            panic!("expected a circle");
        };
        assert!((circle.radius - 4.0).abs() < 1e-9);
        assert!((circle.plane.origin[0] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn spheres_apart_touching_and_identical() {
        let at = |x: f64, radius: f64| {
            Surface::Sphere(Sphere {
                frame: plane_at([x, 0.0, 0.0], [0.0, 0.0, 1.0]),
                radius,
            })
        };
        assert_eq!(surfaces(&at(0.0, 1.0), &at(10.0, 1.0), TOL), Meeting::None);
        assert_eq!(
            surfaces(&at(0.0, 1.0), &at(0.0, 1.0), TOL),
            Meeting::Coincident
        );
        // Nested, never touching.
        assert_eq!(surfaces(&at(0.0, 5.0), &at(0.0, 1.0), TOL), Meeting::None);
        match surfaces(&at(0.0, 1.0), &at(2.0, 1.0), 1e-6) {
            Meeting::Points(points) => assert!((points[0][0] - 1.0).abs() < 1e-9),
            other => panic!("expected a touch, got {other:?}"),
        }
    }

    #[test]
    fn a_plane_square_to_a_cylinder_cuts_a_circle() {
        let cylinder = Surface::Cylinder(Cylinder {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 3.0,
        });
        let plane = Surface::Plane(plane_at([0.0, 0.0, 7.0], [0.0, 0.0, 1.0]));
        let meeting = surfaces(&cylinder, &plane, TOL);
        assert_on_both(&cylinder, &plane, &meeting);
        let Meeting::Curves(curves) = &meeting else {
            unreachable!()
        };
        let Curve3::Circle(circle) = &curves[0] else {
            panic!("expected a circle, got {:?}", curves[0]);
        };
        assert!((circle.radius - 3.0).abs() < 1e-9);
        assert!((circle.plane.origin[2] - 7.0).abs() < 1e-9);
    }

    #[test]
    fn a_slanted_plane_cuts_a_cylinder_in_an_ellipse() {
        let cylinder = Surface::Cylinder(Cylinder {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 3.0,
        });
        // Forty-five degrees, so the major axis is the radius times √2.
        let plane = Surface::Plane(plane_at([0.0; 3], [0.0, 1.0, 1.0]));
        let meeting = surfaces(&cylinder, &plane, TOL);
        assert_on_both(&cylinder, &plane, &meeting);
        let Meeting::Curves(curves) = &meeting else {
            unreachable!()
        };
        let Curve3::Ellipse(ellipse) = &curves[0] else {
            panic!("expected an ellipse, got {:?}", curves[0]);
        };
        assert!((ellipse.minor_radius - 3.0).abs() < 1e-9);
        assert!(
            (ellipse.major_radius - 3.0 * std::f64::consts::SQRT_2).abs() < 1e-9,
            "{}",
            ellipse.major_radius
        );
    }

    #[test]
    fn a_plane_along_a_cylinder_cuts_two_generators() {
        let cylinder = Surface::Cylinder(Cylinder {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 5.0,
        });
        // Parallel to the axis, three units off it: a chord of half-width 4.
        let plane = Surface::Plane(plane_at([3.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        let meeting = surfaces(&cylinder, &plane, TOL);
        assert_on_both(&cylinder, &plane, &meeting);
        let Meeting::Curves(curves) = &meeting else {
            unreachable!()
        };
        assert_eq!(curves.len(), 2);
        for curve in curves {
            let Curve3::Line(line) = curve else {
                panic!("expected lines");
            };
            assert!((line.origin[0] - 3.0).abs() < 1e-9);
            assert!((line.origin[1].abs() - 4.0).abs() < 1e-9, "{line:?}");
        }
    }

    #[test]
    fn a_plane_that_grazes_a_cylinder_gives_one_generator() {
        let cylinder = Surface::Cylinder(Cylinder {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 5.0,
        });
        let plane = Surface::Plane(plane_at([5.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        let meeting = surfaces(&cylinder, &plane, 1e-6);
        assert_on_both(&cylinder, &plane, &meeting);
        let Meeting::Curves(curves) = &meeting else {
            unreachable!()
        };
        assert_eq!(curves.len(), 1);
        let far = Surface::Plane(plane_at([6.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        assert_eq!(surfaces(&cylinder, &far, TOL), Meeting::None);
    }

    #[test]
    fn a_plane_square_to_a_cone_cuts_the_circle_at_that_height() {
        let cone = Surface::Cone(Cone {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 10.0,
            half_angle: FRAC_PI_4,
        });
        let plane = Surface::Plane(plane_at([0.0, 0.0, 4.0], [0.0, 0.0, 1.0]));
        let meeting = surfaces(&cone, &plane, TOL);
        assert_on_both(&cone, &plane, &meeting);
        let Meeting::Curves(curves) = &meeting else {
            unreachable!()
        };
        let Curve3::Circle(circle) = &curves[0] else {
            panic!("expected a circle");
        };
        // Forty-five degrees, so a unit of height costs a unit of radius.
        assert!((circle.radius - 6.0).abs() < 1e-9, "{}", circle.radius);
    }

    #[test]
    fn a_plane_through_a_cone_apex_touches_at_a_point() {
        let cone = Surface::Cone(Cone {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 10.0,
            half_angle: FRAC_PI_4,
        });
        let plane = Surface::Plane(plane_at([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]));
        match surfaces(&cone, &plane, 1e-6) {
            Meeting::Points(points) => assert!((points[0][2] - 10.0).abs() < 1e-9),
            other => panic!("expected the apex, got {other:?}"),
        }
    }

    #[test]
    fn a_slanted_plane_on_a_cone_says_it_does_not_know() {
        // A parabola or a hyperbola has no representation here yet, and
        // returning only the elliptical cases would be a silent gap.
        let cone = Surface::Cone(Cone {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 10.0,
            half_angle: FRAC_PI_4,
        });
        let plane = Surface::Plane(plane_at([0.0; 3], [0.0, 1.0, 1.0]));
        assert_eq!(surfaces(&cone, &plane, TOL), Meeting::Unknown);
    }

    #[test]
    fn parallel_cylinders_share_their_generators() {
        let at = |x: f64, radius: f64| {
            Surface::Cylinder(Cylinder {
                base: plane_at([x, 0.0, 0.0], [0.0, 0.0, 1.0]),
                radius,
            })
        };
        let one = at(0.0, 5.0);
        let other = at(6.0, 5.0);
        let meeting = surfaces(&one, &other, TOL);
        assert_on_both(&one, &other, &meeting);
        let Meeting::Curves(curves) = &meeting else {
            unreachable!()
        };
        assert_eq!(curves.len(), 2);

        assert_eq!(surfaces(&at(0.0, 1.0), &at(10.0, 1.0), TOL), Meeting::None);
        assert_eq!(
            surfaces(&at(0.0, 3.0), &at(0.0, 3.0), TOL),
            Meeting::Coincident
        );
        assert_eq!(surfaces(&at(0.0, 3.0), &at(0.0, 1.0), TOL), Meeting::None);
    }

    #[test]
    fn crossing_cylinders_say_they_are_not_known_rather_than_apart() {
        // The failure the distinction exists for: told "None", a union would
        // put these two side by side and leave the shape they actually share
        // as a hole.
        let upright = Surface::Cylinder(Cylinder {
            base: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 2.0,
        });
        let across = Surface::Cylinder(Cylinder {
            base: plane_at([0.0; 3], [1.0, 0.0, 0.0]),
            radius: 2.0,
        });
        assert_eq!(surfaces(&upright, &across, TOL), Meeting::Unknown);
    }

    #[test]
    fn a_torus_meets_a_plane_square_to_its_axis_in_two_circles_only() {
        let torus = Surface::Torus(super::super::Torus {
            frame: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            major_radius: 10.0,
            minor_radius: 2.0,
        });
        let square = Surface::Plane(plane_at([0.0; 3], [0.0, 0.0, 1.0]));
        let Meeting::Curves(circles) = surfaces(&torus, &square, TOL) else {
            panic!("a plane through the ring is answered");
        };
        let mut radii: Vec<f64> = circles
            .iter()
            .map(|curve| match curve {
                Curve3::Circle(circle) => circle.radius,
                other => panic!("{other:?}"),
            })
            .collect();
        radii.sort_by(f64::total_cmp);
        assert!((radii[0] - 8.0).abs() < 1e-9 && (radii[1] - 12.0).abs() < 1e-9, "{radii:?}");
        // A tilted section is a quartic and stays unknown.
        let tilted = Surface::Plane(plane_at([0.0; 3], [0.0, 1.0, 1.0]));
        assert_eq!(surfaces(&torus, &tilted, TOL), Meeting::Unknown);
    }

    #[test]
    fn the_answer_does_not_depend_on_which_surface_comes_first() {
        let sphere = Surface::Sphere(Sphere {
            frame: plane_at([0.0; 3], [0.0, 0.0, 1.0]),
            radius: 5.0,
        });
        let plane = Surface::Plane(plane_at([0.0, 0.0, 3.0], [0.0, 0.0, 1.0]));
        let one = surfaces(&sphere, &plane, TOL);
        let other = surfaces(&plane, &sphere, TOL);
        assert_eq!(one, other);
    }

    #[test]
    fn survey_coordinates_find_the_same_intersection() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let sphere = Surface::Sphere(Sphere {
            frame: plane_at(origin, [0.0, 0.0, 1.0]),
            radius: 5.0,
        });
        let plane = Surface::Plane(plane_at(
            [origin[0], origin[1], origin[2] + 3.0],
            [0.0, 0.0, 1.0],
        ));
        let meeting = surfaces(&sphere, &plane, 1e-6);
        let Meeting::Curves(curves) = &meeting else {
            panic!("expected a circle, got {meeting:?}");
        };
        let Curve3::Circle(circle) = &curves[0] else {
            unreachable!()
        };
        assert!((circle.radius - 4.0).abs() < 1e-6, "{}", circle.radius);
    }
}
