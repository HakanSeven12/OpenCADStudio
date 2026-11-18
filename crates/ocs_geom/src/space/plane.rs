//! A plane, and the two-way map between its `(u, v)` coordinates and space.
//!
//! # Why two axes and not a normal
//!
//! A normal fixes the plane but not the frame on it: every rotation about
//! the normal describes the same plane, and nothing in the normal says which
//! direction is `u`. Storing only a normal would force whoever converts a 2D
//! coordinate into space to *invent* the missing choice, and that invention
//! is a storage convention rather than geometry.
//!
//! Formats supply it themselves, which is the tell. An ACIS `plane` record
//! carries a root point, a normal, *and* a `u_direction`. DXF instead derives
//! the frame from the normal by a fixed rule — the arbitrary-axis algorithm —
//! which is exactly the kind of format-specific decision this crate should
//! not be making on a caller's behalf: pick differently and coordinates stop
//! round-tripping, so the rule belongs with the reader that knows the format.
//!
//! Two axes also express what a normal cannot. They need not be unit or even
//! perpendicular, so a plane can carry a scaled or sheared parameterisation —
//! which is what a B-rep face's `(u, v)` generally is.
//!
//! Use [`Plane::orthonormal`] when a clean frame is what is wanted; it builds
//! one and reports the degenerate inputs rather than producing a plane that
//! quietly is not one.

use super::vec::Vec3;

const COPLANARITY_TOLERANCE: f64 = 1e-9;

/// Intersection of an infinite line and a plane given by a point and normal.
/// `angular_tolerance` bounds the absolute dot product of the unit directions;
/// parallel, degenerate and nonfinite inputs return `None`.
pub fn intersect_line_plane(
    line_origin: [f64; 3],
    line_direction: [f64; 3],
    plane_origin: [f64; 3],
    plane_normal: [f64; 3],
    angular_tolerance: f64,
) -> Option<[f64; 3]> {
    if !line_origin.iter().chain(&line_direction).chain(&plane_origin).chain(&plane_normal)
        .all(|value| value.is_finite())
        || !angular_tolerance.is_finite() || angular_tolerance < 0.0
    {
        return None;
    }
    let direction = Vec3::from(line_direction).normalize()?;
    let normal = Vec3::from(plane_normal).normalize()?;
    let denominator = direction.dot(normal);
    if denominator.abs() <= angular_tolerance {
        return None;
    }
    // Evaluate near the plane origin, not at a potentially distant line origin.
    let offset = Vec3::from(line_origin) - Vec3::from(plane_origin);
    let nearest = offset - direction * offset.dot(direction);
    let hit = Vec3::from(plane_origin) + (nearest - direction * (nearest.dot(normal) / denominator));
    hit.is_finite().then_some(hit.to_array())
}

/// Scale-aware distance used by coplanarity checks.
pub fn coplanarity_tolerance(points: &[[f64; 3]]) -> f64 {
    if !points.iter().flatten().all(|value| value.is_finite()) {
        return f64::NAN;
    }
    let Some(origin) = points.first().copied() else {
        return COPLANARITY_TOLERANCE;
    };
    let origin = Vec3::from(origin);
    let extent = points
        .iter()
        .map(|point| (Vec3::from(*point) - origin).length())
        .fold(1.0, f64::max);
    let coordinate_scale = points
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    COPLANARITY_TOLERANCE * extent + f64::EPSILON * coordinate_scale * 64.0
}

/// Reorients an axis into the plane perpendicular to `normal` while keeping
/// its original length and as much of its original direction as possible.
pub fn reorient_axis_to_plane(axis: [f64; 3], normal: [f64; 3]) -> Option<[f64; 3]> {
    let axis = Vec3::from(axis);
    let length = axis.length();
    let normal = Vec3::from(normal).normalize()?;
    if !axis.is_finite() || !length.is_finite() || length <= 1.0e-300 {
        return None;
    }
    let projected = axis - normal * axis.dot(normal);
    let direction = projected.normalize().or_else(|| {
        let helper = if normal.x.abs() < 0.8 {
            Vec3::X
        } else {
            Vec3::Y
        };
        normal.cross(helper).normalize()
    })?;
    Some((direction * length).to_array())
}

/// Whether points and directions share a plane.
pub fn are_coplanar(points: &[[f64; 3]], directions: &[[f64; 3]]) -> bool {
    if !points
        .iter()
        .chain(directions)
        .flatten()
        .all(|value| value.is_finite())
    {
        return false;
    }
    let Some(origin) = points.first().copied() else {
        return true;
    };
    let origin = Vec3::from(origin);
    let offsets: Vec<Vec3> = points
        .iter()
        .skip(1)
        .map(|point| Vec3::from(*point) - origin)
        .collect();
    let tolerance = coplanarity_tolerance(points);
    let point_axes = offsets
        .iter()
        .copied()
        .filter(|offset| offset.length() > tolerance)
        .filter_map(Vec3::normalize);
    let direction_axes = directions
        .iter()
        .copied()
        .map(Vec3::from)
        .filter_map(Vec3::normalize);
    let axes: Vec<Vec3> = point_axes.chain(direction_axes).collect();
    let Some(axis) = axes.first().copied()
    else {
        return true;
    };
    let Some(normal) = axes.iter().find_map(|candidate| {
        let cross = axis.cross(*candidate);
        (cross.length() > COPLANARITY_TOLERANCE)
            .then(|| cross.normalize())
            .flatten()
    }) else {
        return true;
    };
    offsets
        .iter()
        .all(|offset| offset.dot(normal).abs() <= tolerance)
        && directions.iter().copied().map(Vec3::from).all(|direction| {
            direction.length_squared() == 0.0
                || direction.dot(normal).abs()
                    <= COPLANARITY_TOLERANCE * direction.length()
        })
}

/// A plane in space, with a coordinate frame on it.
///
/// [`point_at`](Self::point_at) and [`project`](Self::project) are inverses
/// for any point on the plane, whatever the axes are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    /// Where `(0, 0)` lands.
    pub origin: [f64; 3],
    /// The direction `u` advances along, and how far one unit of it goes.
    pub x_axis: [f64; 3],
    /// The same for `v`.
    pub y_axis: [f64; 3],
}

impl Plane {
    /// The world XY plane: the frame the overwhelming majority of drawing
    /// geometry is stored in, and the one a 2D curve means when it does not
    /// say otherwise.
    pub const XY: Self = Self {
        origin: [0.0, 0.0, 0.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    };

    /// A plane from an origin and two axes, taken as given.
    ///
    /// Nothing is normalised or squared up. That is the point: this is the
    /// constructor for a frame that already means something to its source —
    /// a face's parameterisation, a reader's arbitrary-axis result — and
    /// tidying it here would silently move geometry.
    pub const fn from_axes(origin: [f64; 3], x_axis: [f64; 3], y_axis: [f64; 3]) -> Self {
        Self {
            origin,
            x_axis,
            y_axis,
        }
    }

    /// A plane with a unit, right-angled frame: `x_axis` normalised, and the
    /// second axis completing a right-handed pair with `normal`.
    ///
    /// `x_axis` need not already be perpendicular to `normal`; the component
    /// along the normal is removed. `None` when the two are parallel or
    /// either is degenerate, since there is no frame to build from them.
    pub fn orthonormal(origin: [f64; 3], x_axis: [f64; 3], normal: [f64; 3]) -> Option<Self> {
        let normal = Vec3::from(normal).normalize()?;
        let wanted = Vec3::from(x_axis);
        // Project out the normal component first. Without this a caller whose
        // `x_axis` is merely close to the plane gets a frame that is not on
        // it, and the error rides along into every coordinate afterwards.
        let x = (wanted - normal * wanted.dot(normal)).normalize()?;
        Some(Self {
            origin,
            x_axis: x.to_array(),
            y_axis: normal.cross(x).to_array(),
        })
    }

    /// The unit normal, `x × y`.
    ///
    /// `None` when the axes are parallel or degenerate, which is to say when
    /// the "plane" does not span one.
    pub fn normal(&self) -> Option<[f64; 3]> {
        Vec3::from(self.x_axis)
            .cross(Vec3::from(self.y_axis))
            .normalize()
            .map(Vec3::to_array)
    }

    /// Whether the frame is unit and right-angled, so
    /// [`project`](Self::project) reduces to a pair of dot products.
    pub fn is_orthonormal(&self) -> bool {
        const TOLERANCE: f64 = 1e-9;
        let (x, y) = (Vec3::from(self.x_axis), Vec3::from(self.y_axis));
        (x.length_squared() - 1.0).abs() < TOLERANCE
            && (y.length_squared() - 1.0).abs() < TOLERANCE
            && x.dot(y).abs() < TOLERANCE
    }

    /// The point at plane coordinates `(u, v)`.
    pub fn point_at(&self, uv: [f64; 2]) -> [f64; 3] {
        let point = Vec3::from(self.origin) + self.vector_at(uv).into();
        point.to_array()
    }

    /// The same for a direction: the origin is not added, so a tangent stays
    /// a tangent.
    pub fn vector_at(&self, uv: [f64; 2]) -> [f64; 3] {
        (Vec3::from(self.x_axis) * uv[0] + Vec3::from(self.y_axis) * uv[1]).to_array()
    }

    /// The plane coordinates of `point`, the inverse of
    /// [`point_at`](Self::point_at).
    ///
    /// A point off the plane is projected onto it along the normal, which is
    /// what lets a caller hand over a cursor position or an intersection
    /// without checking it first.
    ///
    /// `None` when the axes do not span a plane.
    pub fn project(&self, point: [f64; 3]) -> Option<[f64; 2]> {
        let offset = Vec3::from(point) - Vec3::from(self.origin);
        self.project_vector(offset.to_array())
    }

    /// Plane coordinates at a requested world XY position.
    ///
    /// This intersects the plane with the world-Z line through `xy`. It is
    /// undefined when the plane is edge-on to XY.
    pub fn coordinates_at_xy(&self, xy: [f64; 2]) -> Option<[f64; 2]> {
        if !xy.iter().all(|value| value.is_finite()) {
            return None;
        }
        let [xx, xy_axis, _] = self.x_axis;
        let [yx, yy, _] = self.y_axis;
        let determinant = xx * yy - xy_axis * yx;
        let scale = (xx * xx + xy_axis * xy_axis)
            * (yx * yx + yy * yy);
        if !determinant.is_finite()
            || determinant.abs() <= 1e-24 * scale.max(f64::MIN_POSITIVE)
        {
            return None;
        }
        let dx = xy[0] - self.origin[0];
        let dy = xy[1] - self.origin[1];
        Some([
            (dx * yy - dy * yx) / determinant,
            (dy * xx - dx * xy_axis) / determinant,
        ])
    }

    /// The same for a direction: the origin is not subtracted, so a tangent
    /// or an axis keeps its meaning. The inverse of
    /// [`vector_at`](Self::vector_at).
    pub fn project_vector(&self, vector: [f64; 3]) -> Option<[f64; 2]> {
        // The least-squares solve, not a pair of dot products: those are only
        // the inverse when the frame is orthonormal, and this type exists in
        // part to allow frames that are not.
        let (x, y) = (Vec3::from(self.x_axis), Vec3::from(self.y_axis));
        let offset = Vec3::from(vector);
        let (xx, xy, yy) = (x.length_squared(), x.dot(y), y.length_squared());
        let determinant = xx * yy - xy * xy;
        // Equal to |x × y|², so this is the same degeneracy `normal` reports:
        // parallel or collapsed axes. Scaled against the magnitudes involved,
        // since an absolute floor would reject a legitimately small plane.
        if determinant.abs() <= 1e-24 * (xx * yy).max(f64::MIN_POSITIVE) {
            return None;
        }
        let (dx, dy) = (offset.dot(x), offset.dot(y));
        Some([
            (dx * yy - dy * xy) / determinant,
            (dy * xx - dx * xy) / determinant,
        ])
    }

    /// How far `point` sits off the plane, signed along the normal.
    ///
    /// `None` when the axes do not span a plane.
    pub fn distance_to(&self, point: [f64; 3]) -> Option<f64> {
        let normal = Vec3::from(self.normal()?);
        Some((Vec3::from(point) - Vec3::from(self.origin)).dot(normal))
    }

    /// Whether `point` lies on the plane, to within `tolerance`.
    pub fn contains(&self, point: [f64; 3], tolerance: f64) -> bool {
        self.distance_to(point)
            .is_some_and(|distance| distance.abs() <= tolerance)
    }

    /// Whether this plane is the world XY plane exactly, origin included.
    pub fn is_xy(&self) -> bool {
        *self == Self::XY
    }

    /// Whether the frame is the world XY basis, wherever its origin sits.
    ///
    /// The dominant case: a drawing's planar entities are stored with an
    /// extrusion normal of +Z and differ only in elevation. Mapping through
    /// such a plane is an offset rather than a rotation, so a caller on a hot
    /// path can skip the six multiplies [`point_at`](Self::point_at) costs.
    ///
    /// Compared exactly, not within a tolerance: the axes either came from
    /// the +Z fast path or they did not, and a plane that is merely close to
    /// XY must take the general route or its points would move.
    pub fn is_xy_aligned(&self) -> bool {
        self.x_axis == [1.0, 0.0, 0.0] && self.y_axis == [0.0, 1.0, 0.0]
    }
}

impl Default for Plane {
    fn default() -> Self {
        Self::XY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_1_SQRT_2;

    #[test]
    fn line_plane_intersection_preserves_elevation_and_rejects_invalid_inputs() {
        let plane = [639_792.184_2, 4_517_057.531_7, 12.5];
        let line = [plane[0] + 3.0, plane[1] - 7.0, 181_080.0];
        assert_eq!(intersect_line_plane(line, [0.0, 0.0, -4.0], plane, [0.0, 0.0, 2.0], 1e-6),
            Some([line[0], line[1], plane[2]]));
        assert_eq!(intersect_line_plane([0.0, 0.0, 10.0], [1.0, 0.0, -1.0], [0.0; 3], [0.0, 1.0, 1.0], 1e-6),
            Some([10.0, 0.0, 0.0]));
        for direction in [[1.0, 0.0, 0.0], [1.0, 0.0, 1e-7], [0.0; 3], [f64::NAN, 0.0, 1.0]] {
            assert_eq!(intersect_line_plane(line, direction, plane, [0.0, 0.0, 1.0], 1e-6), None);
        }
        assert_eq!(intersect_line_plane(line, [0.0, 0.0, 1.0], plane, [0.0; 3], 1e-6), None);
        assert_eq!(intersect_line_plane(line, [0.0, 0.0, 1.0], plane, [0.0, 0.0, 1.0], f64::NAN), None);
    }

    #[test]
    fn reoriented_axis_keeps_length_and_lies_in_the_new_plane() {
        for (axis, normal) in [
            ([3.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
            ([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]),
        ] {
            let result = Vec3::from(reorient_axis_to_plane(axis, normal).unwrap());
            let normal = Vec3::from(normal).normalize().unwrap();
            assert!((result.length() - Vec3::from(axis).length()).abs() < 1.0e-12);
            assert!(result.dot(normal).abs() < 1.0e-12);
        }
    }

    /// A plane whose frame is deliberately neither unit nor right-angled.
    fn skewed() -> Plane {
        Plane::from_axes([1.0, 2.0, 3.0], [2.0, 0.0, 0.0], [1.0, 3.0, 0.0])
    }

    #[test]
    fn the_xy_plane_carries_coordinates_through_unchanged() {
        assert_eq!(Plane::XY.point_at([4.0, -7.0]), [4.0, -7.0, 0.0]);
        assert_eq!(Plane::XY.project([4.0, -7.0, 0.0]), Some([4.0, -7.0]));
        assert_eq!(Plane::XY.normal(), Some([0.0, 0.0, 1.0]));
        assert!(Plane::XY.is_xy());
        assert!(Plane::default().is_xy());
    }

    #[test]
    fn projecting_inverts_point_at_on_an_orthonormal_frame() {
        let plane =
            Plane::orthonormal([10.0, 20.0, 30.0], [1.0, 1.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        for uv in [[0.0, 0.0], [3.0, -5.0], [-1.25, 8.5]] {
            let round_tripped = plane.project(plane.point_at(uv)).unwrap();
            assert!((round_tripped[0] - uv[0]).abs() < 1e-12);
            assert!((round_tripped[1] - uv[1]).abs() < 1e-12);
        }
    }

    #[test]
    fn projecting_inverts_point_at_on_a_skewed_frame_too() {
        // The case a pair of dot products would get wrong: with a non-unit,
        // non-perpendicular frame, `offset · x` is not `u`.
        let plane = skewed();
        for uv in [[1.0, 0.0], [0.0, 1.0], [3.0, -5.0], [-1.25, 8.5]] {
            let round_tripped = plane.project(plane.point_at(uv)).unwrap();
            assert!(
                (round_tripped[0] - uv[0]).abs() < 1e-12
                    && (round_tripped[1] - uv[1]).abs() < 1e-12,
                "{uv:?} came back as {round_tripped:?}"
            );
        }
    }

    #[test]
    fn dot_products_would_have_disagreed_on_the_skewed_frame() {
        // Guards the reason `project` solves rather than dots: on this frame
        // the naive answer is wrong by a wide margin, so the test above is
        // testing something.
        let plane = skewed();
        let point = plane.point_at([3.0, -5.0]);
        let offset = Vec3::from(point) - Vec3::from(plane.origin);
        // The coordinate is 3. The dot product says 2 — not a rounding
        // difference but a different answer, because `x` is neither unit nor
        // perpendicular to `y`.
        let naive = offset.dot(Vec3::from(plane.x_axis));
        assert!((naive - 2.0).abs() < 1e-12, "naive u was {naive}");
        assert_eq!(plane.project(point).map(|uv| uv[0].round()), Some(3.0));
    }

    #[test]
    fn a_direction_projects_without_the_origin() {
        // The XZ plane, far from the origin. y = normal × x = −Z, so the
        // plane's own "up" runs down the world Z axis — which is the part a
        // dot-product-free shortcut would get wrong.
        let plane =
            Plane::orthonormal([100.0, 200.0, 300.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        assert_eq!(plane.y_axis, [0.0, 0.0, -1.0]);
        // A vector, not a point: the distant origin must not enter into it.
        assert_eq!(plane.vector_at([3.0, -4.0]), [3.0, 0.0, 4.0]);
        assert_eq!(plane.project_vector([3.0, 0.0, 4.0]), Some([3.0, -4.0]));
        // Skewed frames go through the same solve as `project`.
        let skew = skewed();
        assert_eq!(skew.project_vector(skew.vector_at([2.0, 5.0])), Some([2.0, 5.0]));
    }

    #[test]
    fn a_point_off_the_plane_projects_along_the_normal() {
        let plane = Plane::orthonormal([0.0, 0.0, 5.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        assert_eq!(plane.project([2.0, 3.0, 99.0]), Some([2.0, 3.0]));
        assert_eq!(plane.distance_to([2.0, 3.0, 99.0]), Some(94.0));
        assert_eq!(plane.distance_to([2.0, 3.0, 1.0]), Some(-4.0));
        assert!(plane.contains([2.0, 3.0, 5.0], 1e-9));
        assert!(!plane.contains([2.0, 3.0, 6.0], 1e-9));
    }

    #[test]
    fn orthonormal_squares_up_an_axis_that_leans_off_the_plane() {
        // x has a component along the normal; it should be removed rather
        // than producing a frame that is not on the plane.
        let plane = Plane::orthonormal([0.0; 3], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]).unwrap();
        assert!(plane.is_orthonormal());
        assert!((plane.x_axis[0] - 1.0).abs() < 1e-12);
        assert!(plane.x_axis[2].abs() < 1e-12);
        assert_eq!(plane.normal(), Some([0.0, 0.0, 1.0]));
    }

    #[test]
    fn orthonormal_is_right_handed() {
        let plane = Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        assert_eq!(plane.y_axis, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn orthonormal_refuses_an_axis_along_the_normal() {
        assert!(Plane::orthonormal([0.0; 3], [0.0, 0.0, 2.0], [0.0, 0.0, 1.0]).is_none());
        assert!(Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0]).is_none());
        assert!(Plane::orthonormal([0.0; 3], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]).is_none());
    }

    #[test]
    fn a_collapsed_frame_reports_itself_rather_than_answering() {
        let degenerate = Plane::from_axes([0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]);
        assert!(degenerate.normal().is_none());
        assert!(degenerate.project([1.0, 1.0, 1.0]).is_none());
        assert!(degenerate.distance_to([1.0, 1.0, 1.0]).is_none());
        assert!(!degenerate.contains([0.0; 3], 1e9));
    }

    #[test]
    fn is_orthonormal_tells_the_two_kinds_of_frame_apart() {
        assert!(Plane::XY.is_orthonormal());
        assert!(!skewed().is_orthonormal());
        // Right-angled but not unit is still not orthonormal: the scale is
        // what a face's parameterisation carries, and it must not be assumed
        // away.
        let scaled = Plane::from_axes([0.0; 3], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]);
        assert!(!scaled.is_orthonormal());
        assert_eq!(scaled.normal(), Some([0.0, 0.0, 1.0]));
    }

    #[test]
    fn a_tilted_plane_places_its_own_axes_at_unit_distance() {
        let plane = Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 1.0]).unwrap();
        assert_eq!(plane.point_at([1.0, 0.0]), [1.0, 0.0, 0.0]);
        // y = normal × x, so a normal leaning back towards +Y sends the
        // plane's own "up" forwards along +Y and down along −Z.
        let up = plane.point_at([0.0, 1.0]);
        assert!((up[1] - FRAC_1_SQRT_2).abs() < 1e-12, "{up:?}");
        assert!((up[2] + FRAC_1_SQRT_2).abs() < 1e-12, "{up:?}");
        assert!(plane.contains(up, 1e-12));
    }

    #[test]
    fn survey_coordinates_survive_the_round_trip() {
        let plane = Plane::orthonormal(
            [512_345.678, 4_512_345.678, 91.5],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        )
        .unwrap();
        let uv = [0.001, -0.002];
        let back = plane.project(plane.point_at(uv)).unwrap();
        assert!((back[0] - uv[0]).abs() < 1e-9 && (back[1] - uv[1]).abs() < 1e-9);
    }
}
