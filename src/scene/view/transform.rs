use acadrust::types::{Transform, Vector3};
use glam::DVec3;

use crate::command::EntityTransform;

#[inline]
fn to_v3(v: DVec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

pub fn apply_standard_transform<T>(entity: &mut T, center: DVec3, axis: DVec3, angle_rad: f64)
where
    T: acadrust::Entity,
{
    let t = Transform::from_translation(to_v3(-center))
        .then(&Transform::from_rotation(
            to_v3(axis.normalize_or(DVec3::Z)),
            angle_rad,
        ))
        .then(&Transform::from_translation(to_v3(center)));
    entity.apply_transform(&t);
}

pub fn apply_standard_scale<T>(entity: &mut T, center: DVec3, factor: f64)
where
    T: acadrust::Entity,
{
    let s = factor;
    let t = Transform::from_scaling_with_origin(Vector3::new(s, s, s), to_v3(center));
    entity.apply_transform(&t);
}

pub fn apply_standard_entity_transform<T, F>(entity: &mut T, t: &EntityTransform, mirror: F)
where
    T: acadrust::Entity,
    F: FnOnce(&mut T, DVec3, DVec3),
{
    match t {
        EntityTransform::Translate(d) => entity.translate(to_v3(*d)),
        EntityTransform::Rotate {
            center,
            axis,
            angle_rad,
        } => apply_standard_transform(entity, *center, *axis, *angle_rad),
        EntityTransform::Scale { center, factor } => apply_standard_scale(entity, *center, *factor),
        EntityTransform::Mirror {
            p1,
            p2,
            working_normal,
        } => {
            if working_normal
                .normalize_or(DVec3::Z)
                .abs_diff_eq(DVec3::Z, 1e-10)
            {
                mirror(entity, *p1, *p2)
            } else {
                entity.apply_transform(&reflection_about_working_line(*p1, *p2, *working_normal));
            }
        }
        EntityTransform::Affine(transform) => entity.apply_transform(transform),
    }
}

/// Reflection across the world-XY line through `p1`→`p2` as an acadrust
/// `Transform`, for delegating MIRROR to acadrust's entity-aware
/// `apply_transform` paths (which handle direction flags, stored-angle
/// conventions and bulges themselves). Degenerate line → identity.
pub fn reflection_about_xy_line(p1: DVec3, p2: DVec3) -> acadrust::types::Transform {
    reflection_about_working_line(p1, p2, DVec3::Z)
}

/// Reflection through the plane that contains the picked line and is
/// perpendicular to the active working plane.
pub fn reflection_about_working_line(
    p1: DVec3,
    p2: DVec3,
    working_normal: DVec3,
) -> acadrust::types::Transform {
    use acadrust::types::{Matrix4, Transform};
    let line = p2 - p1;
    let plane_normal = line
        .cross(working_normal.normalize_or(DVec3::Z))
        .normalize_or(DVec3::ZERO);
    if plane_normal.length_squared() < 1e-12 {
        return Transform::identity();
    }
    let n = plane_normal;
    let d = 2.0 * n.dot(p1);
    Transform::from_matrix(Matrix4 {
        m: [
            [
                1.0 - 2.0 * n.x * n.x,
                -2.0 * n.x * n.y,
                -2.0 * n.x * n.z,
                d * n.x,
            ],
            [
                -2.0 * n.y * n.x,
                1.0 - 2.0 * n.y * n.y,
                -2.0 * n.y * n.z,
                d * n.y,
            ],
            [
                -2.0 * n.z * n.x,
                -2.0 * n.z * n.y,
                1.0 - 2.0 * n.z * n.z,
                d * n.z,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ],
    })
}

pub fn reflected_point(p: DVec3, p1: DVec3, p2: DVec3, working_normal: DVec3) -> DVec3 {
    let line = p2 - p1;
    let normal = line
        .cross(working_normal.normalize_or(DVec3::Z))
        .normalize_or(DVec3::ZERO);
    if normal.length_squared() < 1e-12 {
        p
    } else {
        p - 2.0 * normal.dot(p - p1) * normal
    }
}

pub fn reflect_xy_point(x: &mut f64, y: &mut f64, p1: DVec3, p2: DVec3) {
    let ax = (p2.x - p1.x) as f64;
    let ay = (p2.y - p1.y) as f64;
    let len2 = ax * ax + ay * ay;
    if len2 < 1e-12 {
        return;
    }
    let rx = *x - p1.x as f64;
    let ry = *y - p1.y as f64;
    let dot = rx * ax + ry * ay;
    let mx = 2.0 * dot * ax / len2 - rx;
    let my = 2.0 * dot * ay / len2 - ry;
    *x = p1.x as f64 + mx;
    *y = p1.y as f64 + my;
}

/// DXF arbitrary-axis algorithm — returns the OCS X and Y basis vectors in WCS
/// for a given entity normal vector.
///
/// Returns (Ax, Ay) each as (x, y, z) tuples.  When normal ≈ (0,0,1) the
/// function returns the standard basis ((1,0,0), (0,1,0)) without any
/// cross-product computation.
pub fn ocs_axes(normal: (f64, f64, f64)) -> ((f64, f64, f64), (f64, f64, f64)) {
    let (nx, ny, nz) = normal;
    // Fast path: normal is effectively Z-up, OCS = WCS.
    if nx.abs() < 1e-10 && ny.abs() < 1e-10 && (nz - 1.0).abs() < 1e-10 {
        return ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0));
    }
    // Arbitrary-axis algorithm (DXF spec).
    // When |Nx| < 1/64 and |Ny| < 1/64 the normal is near ±Z, so cross with
    // Wy=(0,1,0); otherwise cross with Wz=(0,0,1). The else branch MUST use Wz,
    // not Wx: Wx=(1,0,0)×N collapses to zero for an X-aligned normal (e.g.
    // N=(-1,0,0)), zeroing the OCS axes and mapping every point to the origin —
    // the resulting coincident points then send the arc tessellator into an
    // infinite loop. (#142)
    let (ax_x, ax_y, ax_z) = if nx.abs() < 1.0 / 64.0 && ny.abs() < 1.0 / 64.0 {
        // (0,1,0) × (nx,ny,nz) = (nz, 0, -nx)
        let (x, y, z) = (nz, 0.0, -nx);
        let len = (x * x + y * y + z * z).sqrt().max(1e-12);
        (x / len, y / len, z / len)
    } else {
        // (0,0,1) × (nx,ny,nz) = (-ny, nx, 0)
        let (x, y, z) = (-ny, nx, 0.0);
        let len = (x * x + y * y + z * z).sqrt().max(1e-12);
        (x / len, y / len, z / len)
    };
    // Ay = N × Ax
    let ay_x = ny * ax_z - nz * ax_y;
    let ay_y = nz * ax_x - nx * ax_z;
    let ay_z = nx * ax_y - ny * ax_x;
    ((ax_x, ax_y, ax_z), (ay_x, ay_y, ay_z))
}

/// Transform a single OCS point to WCS using the given normal vector.
#[inline]
pub fn ocs_point_to_wcs(ocs: (f64, f64, f64), normal: (f64, f64, f64)) -> (f64, f64, f64) {
    let (ax, ay) = ocs_axes(normal);
    let (ox, oy, oz) = ocs;
    let (nx, ny, nz) = normal;
    (
        ox * ax.0 + oy * ay.0 + oz * nx,
        ox * ax.1 + oy * ay.1 + oz * ny,
        ox * ax.2 + oy * ay.2 + oz * nz,
    )
}

/// Inverse of [`ocs_point_to_wcs`]: express a WCS point (or direction — the
/// map is a pure rotation, so both use the same math) in the OCS frame of
/// `normal`. Used when writing user edits made in world space (grip drags)
/// back into a planar entity's OCS-stored coordinates.
#[inline]
pub fn wcs_point_to_ocs(wcs: (f64, f64, f64), normal: (f64, f64, f64)) -> (f64, f64, f64) {
    let (ax, ay) = ocs_axes(normal);
    let (wx, wy, wz) = wcs;
    let (nx, ny, nz) = normal;
    (
        wx * ax.0 + wy * ax.1 + wz * ax.2,
        wx * ay.0 + wy * ay.1 + wz * ay.2,
        wx * nx + wy * ny + wz * nz,
    )
}

#[cfg(test)]
mod mirror_delegation_tests {
    use super::*;
    use crate::command::EntityTransform;
    use crate::entities::traits::Transformable;
    use acadrust::entities::hatch::{BoundaryEdge, BoundaryPath, CircularArcEdge, LineEdge};
    use acadrust::entities::Hatch;
    use acadrust::types::Vector2;

    // MIRROR on a hatch goes through reflection_about_xy_line + acadrust's
    // transform_hatch. Mirror a Line→Arc→Line path across a vertical line and
    // assert the arc stays endpoint-continuous, flips its direction flag, and
    // keeps its sweep magnitude (the stored-angle conventions the old
    // hand-rolled closure violated).
    #[test]
    fn hatch_mirror_keeps_arc_continuous_and_sweep() {
        let mut path = BoundaryPath::new();
        path.edges.push(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(0.0, -1.0),
            end: Vector2::new(2.0, 0.0),
        }));
        path.edges.push(BoundaryEdge::CircularArc(CircularArcEdge {
            center: Vector2::new(1.0, 0.0),
            radius: 1.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
            counter_clockwise: true,
        }));
        path.edges.push(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(0.0, 0.0),
            end: Vector2::new(0.0, -1.0),
        }));
        let mut h = Hatch::new();
        h.paths.push(path);

        // Mirror across the vertical line x = 5.
        h.apply_transform(&EntityTransform::Mirror {
            p1: DVec3::new(5.0, 0.0, 0.0),
            p2: DVec3::new(5.0, 1.0, 0.0),
            working_normal: DVec3::Z,
        });

        let edges = &h.paths[0].edges;
        let (l1, arc) = match (&edges[0], &edges[1]) {
            (BoundaryEdge::Line(a), BoundaryEdge::CircularArc(b)) => (a, b),
            _ => panic!("edge kinds changed"),
        };
        assert!(!arc.counter_clockwise, "mirror must flip the flag");
        let sweep = arc.end_angle - arc.start_angle;
        assert!(
            (sweep - std::f64::consts::PI).abs() < 1e-9,
            "stored sweep must stay π, got {sweep}"
        );
        // Stored-angle convention: true point of a CW edge is at -θ.
        let pt = |theta: f64| {
            let a = if arc.counter_clockwise { theta } else { -theta };
            (
                arc.center.x + arc.radius * a.cos(),
                arc.center.y + arc.radius * a.sin(),
            )
        };
        let (sx, sy) = pt(arc.start_angle);
        assert!(
            (sx - l1.end.x).abs() < 1e-9 && (sy - l1.end.y).abs() < 1e-9,
            "arc start {:?} must meet previous line end {:?}",
            (sx, sy),
            (l1.end.x, l1.end.y)
        );
        // Mirror of (2,0) across x=5 is (8,0).
        assert!((sx - 8.0).abs() < 1e-9 && sy.abs() < 1e-9);
    }
}

#[cfg(test)]
mod ocs_axes_142 {
    #[test]
    fn x_normal_axes_nonzero() {
        let (ax, ay) = super::ocs_axes((-1.0, 0.0, 0.0));
        let nz = |v: (f64, f64, f64)| v.0.abs() + v.1.abs() + v.2.abs();
        assert!(nz(ax) > 0.5, "ax collapsed: {:?}", ax);
        assert!(nz(ay) > 0.5, "ay collapsed: {:?}", ay);
        let p = super::ocs_point_to_wcs((-25.0, 90.0, 0.0), (-1.0, 0.0, 0.0));
        assert!(
            p.0.abs() + p.1.abs() + p.2.abs() > 1.0,
            "point collapsed: {:?}",
            p
        );
    }
}
