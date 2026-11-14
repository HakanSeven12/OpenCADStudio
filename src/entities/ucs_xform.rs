//! The UCS ↔ WCS converter: one type that maps points and directions between
//! world coordinates and a user coordinate system.

use document::tables::Ucs;

/// The single bridge between WCS (how geometry and the file are stored) and the
/// active UCS (the coordinate system the user works in). Build one from the
/// tab's active UCS via `DocumentTab::ucs_xform` (in `app`); the
/// `None` UCS yields the identity (plain WCS).
///
/// Every system that has to speak UCS — the coordinate readout, typed input,
/// the UCS icon, snap/ortho, the ViewCube — goes through this one type instead
/// of re-deriving the axis math. Axes are orthonormal, so the inverse rotation
/// is just the transpose (the dot products in `to_ucs`); no matrix inversion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct UcsXform {
    origin: glam::DVec3,
    x: glam::DVec3,
    y: glam::DVec3,
    z: glam::DVec3,
}

impl UcsXform {
    /// Plain WCS — no active UCS.
    pub(crate) fn identity() -> Self {
        Self {
            origin: glam::DVec3::ZERO,
            x: glam::DVec3::X,
            y: glam::DVec3::Y,
            z: glam::DVec3::Z,
        }
    }

    pub(crate) fn from_ucs(ucs: &Ucs) -> Self {
        let v = |a: document::types::Vector3| glam::DVec3::new(a.x, a.y, a.z);
        let x = v(ucs.x_axis).normalize_or(glam::DVec3::X);
        let raw_y = v(ucs.y_axis).normalize_or(glam::DVec3::Y);
        let fallback_z = if x.dot(glam::DVec3::Z).abs() < 0.999 {
            glam::DVec3::Z
        } else {
            glam::DVec3::Y
        };
        let z = x.cross(raw_y).normalize_or(x.cross(fallback_z).normalize());
        let y = z.cross(x).normalize();
        Self {
            origin: v(ucs.origin),
            x,
            y,
            z,
        }
    }

    pub(crate) fn from_active(ucs: Option<&Ucs>) -> Self {
        ucs.map(Self::from_ucs).unwrap_or_else(Self::identity)
    }

    /// True when this is plain WCS — lets callers skip the conversion.
    pub(crate) fn is_identity(&self) -> bool {
        self.origin == glam::DVec3::ZERO
            && self.x == glam::DVec3::X
            && self.y == glam::DVec3::Y
            && self.z == glam::DVec3::Z
    }

    /// UCS point → WCS.
    pub(crate) fn to_wcs(&self, p: glam::DVec3) -> glam::DVec3 {
        self.origin + self.x * p.x + self.y * p.y + self.z * p.z
    }

    /// WCS point → UCS.
    pub(crate) fn to_ucs(&self, p: glam::DVec3) -> glam::DVec3 {
        let d = p - self.origin;
        glam::DVec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }

    /// UCS direction → WCS (rotation only, no origin shift).
    pub(crate) fn vec_to_wcs(&self, v: glam::DVec3) -> glam::DVec3 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    /// WCS direction → UCS (rotation only, no origin shift).
    pub(crate) fn vec_to_ucs(&self, v: glam::DVec3) -> glam::DVec3 {
        glam::DVec3::new(v.dot(self.x), v.dot(self.y), v.dot(self.z))
    }

    /// `(origin, x, y, z)` axes in WCS — for drawing the UCS icon.
    pub(crate) fn axes(&self) -> (glam::DVec3, glam::DVec3, glam::DVec3, glam::DVec3) {
        (self.origin, self.x, self.y, self.z)
    }

    pub(crate) fn working_plane(&self) -> crate::entities::transform::WorkingPlane {
        crate::entities::transform::WorkingPlane::new(self.origin, self.x, self.y)
    }

    /// UCS→world rotation matrix (columns = UCS axes). For consumers that take
    /// a `Mat4` rotation directly (ViewCube, OTRACK ray directions). The GPU /
    /// screen layer is f32, so the axes downcast here at the boundary.
    pub(crate) fn rotation_mat(&self) -> glam::Mat4 {
        glam::Mat4::from_cols(
            self.x.as_vec3().extend(0.0),
            self.y.as_vec3().extend(0.0),
            self.z.as_vec3().extend(0.0),
            glam::Vec4::W,
        )
    }

    /// Full UCS-local → WCS transform, using `origin` as local zero while
    /// retaining this UCS's orthonormal axes.
    pub(crate) fn to_wcs_transform_at(&self, origin: glam::DVec3) -> document::types::Transform {
        use document::types::{Matrix4, Transform};
        Transform::from_matrix(Matrix4 {
            m: [
                [self.x.x, self.y.x, self.z.x, origin.x],
                [self.x.y, self.y.y, self.z.y, origin.y],
                [self.x.z, self.y.z, self.z.z, origin.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        })
    }

    /// Full WCS → UCS-local transform, using `origin` as the local zero.
    pub(crate) fn to_ucs_transform_at(&self, origin: glam::DVec3) -> document::types::Transform {
        use document::types::{Matrix4, Transform};
        Transform::from_matrix(Matrix4 {
            m: [
                [self.x.x, self.x.y, self.x.z, -origin.dot(self.x)],
                [self.y.x, self.y.y, self.y.z, -origin.dot(self.y)],
                [self.z.x, self.z.y, self.z.z, -origin.dot(self.z)],
                [0.0, 0.0, 0.0, 1.0],
            ],
        })
    }

    /// Convert from the represented UCS into its canonical local frame.
    pub(crate) fn to_ucs_transform(&self) -> document::types::Transform {
        self.to_ucs_transform_at(self.origin)
    }
}

#[cfg(test)]
mod ucs_xform_tests {
    use super::*;
    use glam::DVec3;

    const EPS: f64 = 1e-9;

    // A UCS that is both moved and tilted. The axes are written out by hand,
    // so the expected values below do not come from the code under test, and
    // all nine components are non-zero, so a dropped or swapped component
    // cannot hide behind a zero.
    const ORIGIN: DVec3 = DVec3::new(10.0, -20.0, 5.0);
    const X: DVec3 = DVec3::new(1.0 / 9.0, 4.0 / 9.0, 8.0 / 9.0);
    const Y: DVec3 = DVec3::new(4.0 / 9.0, 7.0 / 9.0, -4.0 / 9.0);
    const Z: DVec3 = DVec3::new(-8.0 / 9.0, 4.0 / 9.0, -1.0 / 9.0);

    // The UCS point (1, 2, 3), the same offset as a WCS direction
    // (1·X + 2·Y + 3·Z = (-15, 30, -3) / 9), and the WCS point it names
    // (ORIGIN plus that direction). All worked out by hand.
    const LOCAL: DVec3 = DVec3::new(1.0, 2.0, 3.0);
    const TURNED: DVec3 = DVec3::new(-5.0 / 3.0, 10.0 / 3.0, -1.0 / 3.0);
    const WORLD: DVec3 = DVec3::new(25.0 / 3.0, -50.0 / 3.0, 14.0 / 3.0);

    #[track_caller]
    fn assert_close(actual: DVec3, expected: DVec3, what: &str) {
        assert!(
            (actual - expected).length() < EPS,
            "{what}: expected {expected:?}, got {actual:?}"
        );
    }

    fn vector3(v: DVec3) -> document::types::Vector3 {
        document::types::Vector3::new(v.x, v.y, v.z)
    }

    fn dvec3(v: document::types::Vector3) -> DVec3 {
        DVec3::new(v.x, v.y, v.z)
    }

    fn ucs(origin: DVec3, x: DVec3, y: DVec3) -> Ucs {
        let mut ucs = Ucs::new("TEST");
        ucs.origin = vector3(origin);
        ucs.x_axis = vector3(x);
        ucs.y_axis = vector3(y);
        ucs
    }

    fn tilted() -> UcsXform {
        UcsXform::from_ucs(&ucs(ORIGIN, X, Y))
    }

    #[track_caller]
    fn assert_right_handed_orthonormal(xf: &UcsXform) {
        let (_, x, y, z) = xf.axes();
        for (name, axis) in [("X", x), ("Y", y), ("Z", z)] {
            assert!(
                (axis.length() - 1.0).abs() < EPS,
                "{name} is not unit: {axis:?}"
            );
        }
        assert!(x.dot(y).abs() < EPS, "X and Y are not perpendicular");
        assert_close(x.cross(y), z, "Z is X cross Y");
    }

    /// The fixture has to be a valid frame, and the hand-worked values have
    /// to follow from it, or every expectation below is wrong. This uses
    /// plain vector arithmetic only, not `UcsXform`.
    #[test]
    fn the_fixture_is_consistent() {
        for (name, axis) in [("X", X), ("Y", Y), ("Z", Z)] {
            assert!((axis.length() - 1.0).abs() < EPS, "{name} is not unit");
            for (part, value) in [("x", axis.x), ("y", axis.y), ("z", axis.z)] {
                assert!(value.abs() > 0.1, "{name}.{part} is too close to zero");
            }
        }
        assert!(X.dot(Y).abs() < EPS, "X and Y are not perpendicular");
        assert_close(X.cross(Y), Z, "Z is X cross Y");
        assert_close(X * LOCAL.x + Y * LOCAL.y + Z * LOCAL.z, TURNED, "TURNED");
        assert_close(ORIGIN + TURNED, WORLD, "WORLD");
    }

    #[test]
    fn identity_is_plain_wcs() {
        let xf = UcsXform::identity();
        assert!(xf.is_identity());
        assert_eq!(
            xf.axes(),
            (DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z),
            "the identity is the world frame"
        );
        assert_eq!(xf.to_wcs(LOCAL), LOCAL);
        assert_eq!(xf.to_ucs(LOCAL), LOCAL);
        assert_eq!(xf.vec_to_wcs(LOCAL), LOCAL);
        assert_eq!(xf.vec_to_ucs(LOCAL), LOCAL);
    }

    #[test]
    fn no_active_ucs_is_the_identity() {
        assert_eq!(UcsXform::from_active(None), UcsXform::identity());
        assert_eq!(
            UcsXform::from_active(Some(&ucs(ORIGIN, X, Y))),
            tilted(),
            "an active UCS is used as given"
        );
    }

    #[test]
    fn a_world_aligned_ucs_at_the_origin_is_the_identity() {
        let world = UcsXform::from_ucs(&ucs(DVec3::ZERO, DVec3::X, DVec3::Y));
        assert!(world.is_identity());
        assert_eq!(world, UcsXform::identity());
    }

    /// `is_identity` lets callers skip the conversion, so a UCS that is only
    /// moved, or only turned, must not pass for the world frame.
    #[test]
    fn a_moved_or_turned_ucs_is_not_the_identity() {
        let moved = UcsXform::from_ucs(&ucs(ORIGIN, DVec3::X, DVec3::Y));
        let turned = UcsXform::from_ucs(&ucs(DVec3::ZERO, DVec3::Y, DVec3::NEG_X));
        assert!(!moved.is_identity(), "moved only");
        assert!(!turned.is_identity(), "turned only");
        assert!(!tilted().is_identity(), "moved and tilted");
    }

    #[test]
    fn axes_report_the_ucs_origin_and_axes() {
        let (origin, x, y, z) = tilted().axes();
        assert_close(origin, ORIGIN, "origin");
        assert_close(x, X, "X");
        assert_close(y, Y, "Y");
        assert_close(z, Z, "Z");
    }

    /// A stored UCS need not be clean: axes can be any length, and Y only has
    /// to lie somewhere in the XY plane.
    #[test]
    fn from_ucs_normalises_and_squares_up_the_axes() {
        let scaled = UcsXform::from_ucs(&ucs(ORIGIN, X * 3.0, Y * 0.5));
        let skewed = UcsXform::from_ucs(&ucs(ORIGIN, X, Y + X * 0.7));
        for (name, xf) in [("scaled", scaled), ("skewed", skewed)] {
            let (_, x, y, z) = xf.axes();
            assert_close(x, X, &format!("{name} X"));
            assert_close(y, Y, &format!("{name} Y"));
            assert_close(z, Z, &format!("{name} Z"));
        }
    }

    /// Axes that do not span a plane still have to give a usable frame, with
    /// X kept as given wherever it has a direction.
    #[test]
    fn from_ucs_survives_degenerate_axes() {
        let parallel = UcsXform::from_ucs(&ucs(ORIGIN, X, X * 2.0));
        assert_right_handed_orthonormal(&parallel);
        assert_close(parallel.axes().1, X, "parallel axes keep X");

        let parallel_along_z = UcsXform::from_ucs(&ucs(ORIGIN, DVec3::Z, DVec3::Z));
        assert_right_handed_orthonormal(&parallel_along_z);
        assert_close(parallel_along_z.axes().1, DVec3::Z, "axes along Z keep X");

        let zero_y = UcsXform::from_ucs(&ucs(ORIGIN, X, DVec3::ZERO));
        assert_right_handed_orthonormal(&zero_y);
        assert_close(zero_y.axes().1, X, "a zero Y keeps X");

        let zero_axes = UcsXform::from_ucs(&ucs(ORIGIN, DVec3::ZERO, DVec3::ZERO));
        assert_right_handed_orthonormal(&zero_axes);
        assert_close(zero_axes.axes().0, ORIGIN, "zero axes keep the origin");
    }

    #[test]
    fn points_convert_between_ucs_and_wcs() {
        let xf = tilted();
        assert_close(xf.to_wcs(LOCAL), WORLD, "UCS point to WCS");
        assert_close(xf.to_ucs(WORLD), LOCAL, "WCS point to UCS");
        assert_close(xf.to_wcs(DVec3::ZERO), ORIGIN, "local zero is the origin");
        assert_close(xf.to_ucs(ORIGIN), DVec3::ZERO, "the origin is local zero");
    }

    /// Checked relative to the size of the coordinates, at one part in 10^12,
    /// so that survey-scale points keep sub-micron precision.
    #[test]
    fn point_conversions_undo_each_other() {
        let xf = tilted();
        for p in [
            DVec3::ZERO,
            LOCAL,
            DVec3::new(-7.5, 0.25, 1.0e6),
            DVec3::new(500_000.0, 4_000_000.0, -12.0),
        ] {
            let tolerance = 1e-12 * p.length().max(1.0);
            let via_wcs = xf.to_ucs(xf.to_wcs(p));
            let via_ucs = xf.to_wcs(xf.to_ucs(p));
            assert!(
                (via_wcs - p).length() < tolerance,
                "{p:?} came back as {via_wcs:?}"
            );
            assert!(
                (via_ucs - p).length() < tolerance,
                "{p:?} came back as {via_ucs:?}"
            );
        }
    }

    /// Directions turn with the axes but never pick up the origin: this is
    /// what keeps a relative offset relative.
    #[test]
    fn directions_turn_but_do_not_move() {
        let xf = tilted();
        assert_close(xf.vec_to_wcs(LOCAL), TURNED, "UCS direction to WCS");
        assert_close(xf.vec_to_ucs(TURNED), LOCAL, "WCS direction to UCS");
        assert_close(xf.vec_to_wcs(DVec3::ZERO), DVec3::ZERO, "zero to WCS");
        assert_close(xf.vec_to_ucs(DVec3::ZERO), DVec3::ZERO, "zero to UCS");

        let same_axes_elsewhere = UcsXform::from_ucs(&ucs(DVec3::new(1.0, 1.0, 1.0), X, Y));
        assert_close(
            same_axes_elsewhere.vec_to_wcs(LOCAL),
            TURNED,
            "another origin, to WCS",
        );
        assert_close(
            same_axes_elsewhere.vec_to_ucs(TURNED),
            LOCAL,
            "another origin, to UCS",
        );
    }

    #[test]
    fn working_plane_is_the_same_frame() {
        let xf = tilted();
        let plane = xf.working_plane();
        assert_close(plane.origin, ORIGIN, "origin");
        assert_close(plane.x, X, "X");
        assert_close(plane.y, Y, "Y");
        assert_close(plane.z, Z, "Z");
        assert_close(plane.to_world(LOCAL), xf.to_wcs(LOCAL), "to world");
        assert_close(plane.to_local(WORLD), xf.to_ucs(WORLD), "to local");
    }

    /// The matrix handed to the f32 screen layer: its columns are the axes
    /// and it carries no translation.
    #[test]
    fn rotation_mat_has_the_axes_as_columns() {
        let m = tilted().rotation_mat();
        assert!(
            m.x_axis.abs_diff_eq(X.as_vec3().extend(0.0), 1e-6),
            "X column"
        );
        assert!(
            m.y_axis.abs_diff_eq(Y.as_vec3().extend(0.0), 1e-6),
            "Y column"
        );
        assert!(
            m.z_axis.abs_diff_eq(Z.as_vec3().extend(0.0), 1e-6),
            "Z column"
        );
        assert_eq!(m.w_axis, glam::Vec4::W, "no translation");
        let turned = m.transform_vector3(LOCAL.as_vec3());
        assert!(
            turned.abs_diff_eq(TURNED.as_vec3(), 1e-5),
            "expected {TURNED:?}, got {turned:?}"
        );
    }

    /// The pair a block definition stores: local zero is the picked base
    /// point, not the UCS origin, and the axes are the UCS axes.
    #[test]
    fn transforms_at_a_base_point_use_the_ucs_axes() {
        let xf = tilted();
        let base = DVec3::new(3.0, 4.0, 5.0);
        let world = base + TURNED;

        let to_wcs = xf.to_wcs_transform_at(base);
        let to_ucs = xf.to_ucs_transform_at(base);
        let apply = |t: &document::types::Transform, p: DVec3| dvec3(t.apply(vector3(p)));
        let turn = |t: &document::types::Transform, v: DVec3| dvec3(t.apply_rotation(vector3(v)));

        assert_close(apply(&to_wcs, LOCAL), world, "local point to WCS");
        assert_close(apply(&to_ucs, world), LOCAL, "WCS point to local");
        assert_close(apply(&to_wcs, DVec3::ZERO), base, "local zero is the base");
        assert_close(apply(&to_ucs, base), DVec3::ZERO, "the base is local zero");

        // The rotation part alone turns directions the way `vec_to_*` does.
        assert_close(turn(&to_wcs, LOCAL), TURNED, "local direction to WCS");
        assert_close(turn(&to_ucs, TURNED), LOCAL, "WCS direction to local");
    }

    #[test]
    fn transforms_at_a_base_point_undo_each_other() {
        let xf = tilted();
        let base = DVec3::new(3.0, 4.0, 5.0);
        let to_wcs = xf.to_wcs_transform_at(base);
        let to_ucs = xf.to_ucs_transform_at(base);
        for p in [LOCAL, DVec3::ZERO, DVec3::new(-40.0, 12.5, 7.0)] {
            let there_and_back = to_ucs.apply(to_wcs.apply(vector3(p)));
            assert_close(dvec3(there_and_back), p, "to WCS, then back");
            let back_and_there = to_wcs.apply(to_ucs.apply(vector3(p)));
            assert_close(dvec3(back_and_there), p, "to UCS, then back");
        }
    }

    #[test]
    fn to_ucs_transform_matches_to_ucs() {
        let xf = tilted();
        let transform = xf.to_ucs_transform();
        let apply = |p: DVec3| dvec3(transform.apply(vector3(p)));
        assert_close(apply(WORLD), LOCAL, "a known point");
        assert_close(apply(ORIGIN), DVec3::ZERO, "the origin is local zero");
        for p in [WORLD, ORIGIN, DVec3::new(-3.0, 8.0, 0.5)] {
            assert_close(apply(p), xf.to_ucs(p), "same as to_ucs");
        }
    }
}
