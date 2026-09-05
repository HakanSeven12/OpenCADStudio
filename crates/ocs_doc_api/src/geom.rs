//! Kernel geometry helpers (host feature, plan §10 code-mapping table). Thin
//! wrappers over `cadkernel::brep::*` mapping the crate's plain-data DTOs to the
//! non-serde kernel types. Kernel keys never cross the API (decision #7).

use cadkernel::geom2d::Curve as Curve2;
use cadkernel::space::Plane;

use crate::backend::KernelBody;
use crate::error::{ApiError, ApiResult, GeometryErrorKind};
use crate::ops::{BoolOp, SolidPrimitive};

fn geom_err(kind: GeometryErrorKind, msg: impl Into<String>) -> ApiError {
    ApiError::geometry(kind, msg)
}

/// The XY work plane (origin at 0, X=(1,0,0), normal=(0,0,1)).
fn xy_plane() -> ApiResult<Plane> {
    Plane::orthonormal([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0])
        .ok_or_else(|| geom_err(GeometryErrorKind::Other, "failed to construct XY plane"))
}

/// Construct a solid primitive (plan mapping table).
pub(crate) fn make_solid(p: &SolidPrimitive) -> ApiResult<KernelBody> {
    use cadkernel::brep::make;
    let body = match *p {
        SolidPrimitive::Cuboid { origin, size } => make::cuboid(origin, size),
        SolidPrimitive::Sphere { centre, radius } => make::sphere(centre, radius),
        SolidPrimitive::Cylinder { base, radius, height } => make::cylinder(base, radius, height),
        SolidPrimitive::Cone { base, radius, height } => make::cone(base, radius, height),
        SolidPrimitive::Torus { centre, major_radius, minor_radius } => {
            make::torus(centre, major_radius, minor_radius)
        }
        SolidPrimitive::Wedge { origin, size } => make::wedge(origin, size[0], size[1], size[2]),
    };
    body.ok_or_else(|| geom_err(GeometryErrorKind::InvalidInput, "kernel make returned empty body"))
}

/// Boolean combine (pure): `brep::combine(a, b, op, operation_tolerance(&[&a,&b]))`.
/// Maps the kernel `Snag` to `ApiError::Geometry`. Does NOT mutate the inputs.
pub(crate) fn boolean(a: &KernelBody, b: &KernelBody, op: BoolOp) -> ApiResult<KernelBody> {
    let kernel_op = crate::executor::kernel_bool_op(op);
    let tol = cadkernel::brep::operation_tolerance(&[a, b]);
    cadkernel::brep::combine(a.clone(), b.clone(), kernel_op, tol)
        .map_err(|e| geom_err(GeometryErrorKind::BooleanFailed, format!("{e:?}")))
}

/// Extrude a closed 2D profile into a solid (`brep::extrude`).
pub(crate) fn extrude(profile: &[Curve2], direction: [f64; 3]) -> ApiResult<KernelBody> {
    cadkernel::brep::extrude(xy_plane()?, profile, direction)
        .ok_or_else(|| geom_err(GeometryErrorKind::InvalidInput, "extrude failed"))
}

/// Revolve a closed 2D profile about an axis into a solid (`brep::revolve`).
pub(crate) fn revolve(profile: &[Curve2], pivot: [f64; 3], axis: [f64; 3], angle: f64) -> ApiResult<KernelBody> {
    cadkernel::brep::revolve(xy_plane()?, profile, pivot, axis, angle)
        .ok_or_else(|| geom_err(GeometryErrorKind::InvalidInput, "revolve failed"))
}

