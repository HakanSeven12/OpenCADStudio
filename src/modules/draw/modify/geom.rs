//! Adapters between the editing commands and `opencadkernel`'s plane geometry.
//!
//! The kernel takes points as `[f64; 2]` and hands tessellation back as
//! `[f64; 3]`. The commands here grew up passing loose `x, y` scalars and
//! feeding [`WireModel::points`](crate::scene::model::wire_model::WireModel),
//! which is `[f32; 3]`. These wrappers absorb that difference in one place so
//! the maths itself lives once, in the kernel.
//!
//! Narrowing to `f32` happens here rather than in the kernel deliberately:
//! preview wires leave `points_low` empty, so they accept the loss, while the
//! resident render path splits the `f64` into a high/low pair instead. Only a
//! caller knows which of the two it is.

use kernel::geom2d::{self, Ellipse};

/// Re-exported unchanged: these already speak in plain `f64`, so there is no
/// call-shape difference for this module to absorb.
pub use kernel::geom2d::{arc_parameter, lerp, normalize_angle};

/// Preview geometry keeps the density the commands have always used.
const SEGMENTS_PER_RADIAN: f64 = geom2d::DEFAULT_SEGMENTS_PER_RADIAN;

fn narrow(points: Vec<[f64; 3]>) -> Vec<[f32; 3]> {
    points
        .into_iter()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect()
}

/// Where two infinite lines cross, as `(t, u)`; `None` when parallel.
///
/// Both parameters are unbounded, so EXTEND can use a crossing that lies past
/// the end of either line.
#[allow(clippy::too_many_arguments)]
pub fn line_line(
    px: f64,
    py: f64,
    dx: f64,
    dy: f64,
    qx: f64,
    qy: f64,
    ex: f64,
    ey: f64,
) -> Option<(f64, f64)> {
    geom2d::line_line([px, py], [dx, dy], [qx, qy], [ex, ey])
}

pub fn line_points(start: [f64; 3], end: [f64; 3]) -> Vec<[f32; 3]> {
    narrow(vec![start, end])
}

/// A circular arc sampled counter-clockwise, as render vertices.
pub fn arc_points(cx: f64, cy: f64, r: f64, a0: f64, a1: f64, z: f64) -> Vec<[f32; 3]> {
    narrow(geom2d::arc([cx, cy], r, a0, a1, z, SEGMENTS_PER_RADIAN))
}

/// An elliptical arc sampled between two of the ellipse's own parameters, as
/// render vertices.
///
/// Components come back as `[x, y, z]`. The hand-rolled version this replaces
/// wrote them as `[x, z, y]`, which flattened ELLIPSE trim previews onto
/// `y = 0` and displaced them along Z.
#[allow(clippy::too_many_arguments)]
pub fn ellipse_points(
    cx: f64,
    cy: f64,
    a: f64,
    b: f64,
    nx: f64,
    ny: f64,
    t0: f64,
    t1: f64,
    z: f64,
) -> Vec<[f32; 3]> {
    let ellipse = Ellipse {
        centre: [cx, cy],
        major_radius: a,
        minor_radius: b,
        major_axis: [nx, ny],
    };
    narrow(geom2d::ellipse_arc(
        &ellipse,
        t0,
        t1,
        z,
        SEGMENTS_PER_RADIAN,
    ))
}
