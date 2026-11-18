//! A vector in space.
//!
//! The counterpart to [`Vec2`](crate::geom2d::Vec2), with the same shape and
//! the same reasoning behind it, differing only where the dimension does.
//!
//! # Why this is separate from `Vec2` rather than a generalisation of it
//!
//! The cross product. Here it yields a *vector* — the plane normal, the face
//! orientation, the direction a loop winds around. In the plane it yields a
//! *scalar*, the signed area. Those are different operations that happen to
//! share a name, and a single type would have to pick one and make the other
//! read a component off a mostly-zero result.
//!
//! The dimension is also load-bearing in the type system here: a curve in a
//! face's `(u, v)` space genuinely has no third component, and offering one
//! invites something to be put there. Keeping the plane two-dimensional is
//! how [`PlanarCurve`](super::PlanarCurve) can promise that its curve lives
//! in its plane.
//!
//! # Why this is not the public API
//!
//! Same as `Vec2`: signatures stay `[f64; 3]`. A caller writes a literal,
//! imports nothing, converts for free, and takes on no version pin.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// A point or direction in space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// First component.
    pub x: f64,
    /// Second component.
    pub y: f64,
    /// Third component.
    pub z: f64,
}

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// The positive X axis.
    pub const X: Self = Self::new(1.0, 0.0, 0.0);

    /// The positive Y axis.
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);

    /// The positive Z axis.
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);

    /// A vector from its components.
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// As a plain array, which is what this crate's signatures speak in.
    pub const fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    /// The dot product.
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// The cross product, which in space is a vector.
    ///
    /// Perpendicular to both inputs, right-handed, with length equal to the
    /// area of the parallelogram they span. Zero when they are parallel,
    /// which is the degeneracy every construction here has to check for.
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Length squared. Prefer this to [`length`](Self::length) for
    /// comparisons — it answers the same question without a square root.
    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }

    /// Length.
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    /// Distance to `other`.
    pub fn distance(self, other: Self) -> f64 {
        (other - self).length()
    }

    /// Distance to `other`, squared.
    pub fn distance_squared(self, other: Self) -> f64 {
        (other - self).length_squared()
    }

    /// Whether every component is finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// Distance from this point to a segment.
    pub fn distance_to_segment(self, start: Self, end: Self) -> f64 {
        let along = end - start;
        let squared = along.length_squared();
        if squared < 1e-24 {
            return self.distance(start);
        }
        let parameter = ((self - start).dot(along) / squared).clamp(0.0, 1.0);
        self.distance(start + along * parameter)
    }

    /// Perpendicular distance from this point to an infinite line.
    /// Degenerate and non-finite inputs do not define a line.
    pub fn distance_to_line(self, start: Self, end: Self) -> Option<f64> {
        if !self.is_finite() || !start.is_finite() || !end.is_finite() {
            return None;
        }
        let direction = (end - start).normalize()?;
        let offset = self - start;
        Some((offset - direction * offset.dot(direction)).length())
    }

    /// A unit vector in the same direction.
    ///
    /// `None` when there is no direction to speak of. Returning a zero vector
    /// instead would let a degenerate input travel silently into whatever came
    /// next, which is exactly where a division by nothing turns into geometry
    /// nobody can explain.
    pub fn normalize(self) -> Option<Self> {
        let length = self.length();
        (length > 1e-300).then(|| self / length)
    }

    /// A fraction `t` of the way to `other`.
    pub fn lerp(self, other: Self, t: f64) -> Self {
        self + (other - self) * t
    }

    /// Whether this and `other` point along the same line, to within `tolerance`
    /// on the sine of the angle between them.
    ///
    /// Measured on the normalised pair, so the answer does not depend on how
    /// long either vector happens to be. A zero vector is parallel to nothing.
    pub fn is_parallel_to(self, other: Self, tolerance: f64) -> bool {
        match (self.normalize(), other.normalize()) {
            (Some(a), Some(b)) => a.cross(b).length() <= tolerance,
            _ => false,
        }
    }
}

impl From<[f64; 3]> for Vec3 {
    fn from(value: [f64; 3]) -> Self {
        Self::new(value[0], value[1], value[2])
    }
}

impl From<Vec3> for [f64; 3] {
    fn from(value: Vec3) -> Self {
        value.to_array()
    }
}

impl From<(f64, f64, f64)> for Vec3 {
    fn from(value: (f64, f64, f64)) -> Self {
        Self::new(value.0, value.1, value.2)
    }
}

impl From<Vec3> for (f64, f64, f64) {
    fn from(value: Vec3) -> Self {
        (value.x, value.y, value.z)
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }
}

impl Div<f64> for Vec3 {
    type Output = Self;
    fn div(self, divisor: f64) -> Self {
        Self::new(self.x / divisor, self.y / divisor, self.z / divisor)
    }
}

impl Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrays_and_tuples_convert_both_ways_unchanged() {
        let array = [3.0, -4.0, 12.0];
        let vector = Vec3::from(array);
        assert_eq!((vector.x, vector.y, vector.z), (3.0, -4.0, 12.0));
        assert_eq!(<[f64; 3]>::from(vector), array);
        assert_eq!(Vec3::from((3.0, -4.0, 12.0)), vector);
        assert_eq!(<(f64, f64, f64)>::from(vector), (3.0, -4.0, 12.0));
    }

    #[test]
    fn arithmetic_is_componentwise() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(10.0, 20.0, 30.0);
        assert_eq!(a + b, Vec3::new(11.0, 22.0, 33.0));
        assert_eq!(b - a, Vec3::new(9.0, 18.0, 27.0));
        assert_eq!(a * 3.0, Vec3::new(3.0, 6.0, 9.0));
        assert_eq!(b / 2.0, Vec3::new(5.0, 10.0, 15.0));
        assert_eq!(-a, Vec3::new(-1.0, -2.0, -3.0));
    }

    #[test]
    fn the_cross_product_is_right_handed() {
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
        assert_eq!(Vec3::Z.cross(Vec3::X), Vec3::Y);
        // Reversing the operands reverses the result, unlike the dot product.
        assert_eq!(Vec3::Y.cross(Vec3::X), -Vec3::Z);
    }

    #[test]
    fn the_cross_product_is_perpendicular_to_both_inputs() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(-4.0, 5.0, 6.0);
        let n = a.cross(b);
        assert!(n.dot(a).abs() < 1e-12);
        assert!(n.dot(b).abs() < 1e-12);
    }

    #[test]
    fn parallel_vectors_cross_to_nothing() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(a.cross(a * 2.5), Vec3::ZERO);
    }

    #[test]
    fn length_and_distance_agree() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(1.0, 6.0, 6.0);
        assert_eq!((b - a).length(), 5.0);
        assert_eq!(a.distance(b), 5.0);
        assert_eq!(a.distance_squared(b), 25.0);
    }

    #[test]
    fn distance_to_line_uses_the_infinite_support_and_rejects_a_point() {
        let start = Vec3::new(1.0, 2.0, 3.0);
        let end = Vec3::new(3.0, 2.0, 3.0);
        assert_eq!(
            Vec3::new(100.0, 6.0, 3.0).distance_to_line(start, end),
            Some(4.0)
        );
        assert_eq!(start.distance_to_line(start, start), None);
    }

    #[test]
    fn normalising_gives_a_unit_vector() {
        let unit = Vec3::new(0.0, 3.0, 4.0).normalize().unwrap();
        assert!((unit.length() - 1.0).abs() < 1e-15);
        assert!((unit.y - 0.6).abs() < 1e-15);
    }

    #[test]
    fn a_vector_with_no_direction_refuses_to_normalise() {
        assert!(Vec3::ZERO.normalize().is_none());
    }

    #[test]
    fn parallel_is_measured_on_direction_not_length() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        assert!(a.is_parallel_to(a * 1e6, 1e-12));
        assert!(a.is_parallel_to(-a, 1e-12), "antiparallel is still a line");
        assert!(!a.is_parallel_to(Vec3::new(3.0, 2.0, 1.0), 1e-12));
        assert!(!a.is_parallel_to(Vec3::ZERO, 1.0), "nothing has no direction");
    }

    #[test]
    fn lerp_hits_both_ends_and_the_middle() {
        let a = Vec3::ZERO;
        let b = Vec3::new(10.0, 20.0, 30.0);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
        assert_eq!(a.lerp(b, 0.5), Vec3::new(5.0, 10.0, 15.0));
    }

    #[test]
    fn survey_coordinates_subtract_without_losing_the_difference() {
        let a = Vec3::new(512_345.678, 4_512_345.678, 91.5);
        let b = a + Vec3::new(0.001, 0.0, 0.0);
        assert!((a.distance(b) - 0.001).abs() < 1e-9);
    }
}
