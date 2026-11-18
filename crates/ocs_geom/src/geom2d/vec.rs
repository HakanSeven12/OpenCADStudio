//! A vector in the plane.
//!
//! The arithmetic every other module here was writing out by hand: the
//! squared-length expression alone appeared thirteen times across six files,
//! and only one of them had a `distance` function to call.
//!
//! # Why this is not `Vec3` with a zero
//!
//! The cross product is a different operation in the plane. Here it produces a
//! *scalar* — the signed area, which is what decides orientation, winding, and
//! which side of a curve a pick landed on. In space it produces a vector.
//! Folding the two together would make every 2D caller read a component off a
//! result that is two-thirds zeros, and would hide that they are asking
//! different questions.
//!
//! The 2D layer is also genuinely two-dimensional: splitting a face during a
//! boolean happens in that face's `(u, v)` space, where there is no third
//! component to hold anything. Offering one invites something to be put there.
//!
//! # Why this is not the public API
//!
//! Signatures across the crate stay `[f64; 2]`. A caller can write a literal,
//! needs no import, and converts for free — the two have the same layout, and
//! [`From`] goes both ways. Keeping the boundary plain also means no consumer
//! is pinned to a version of anything on this crate's account.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// A point or direction in the plane.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    /// First component.
    pub x: f64,
    /// Second component.
    pub y: f64,
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    /// A vector from its components.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// As a plain array, which is what this crate's signatures speak in.
    pub const fn to_array(self) -> [f64; 2] {
        [self.x, self.y]
    }

    /// The dot product.
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// The cross product, which in the plane is a signed scalar.
    ///
    /// Twice the signed area of the triangle the two vectors span. Positive
    /// when `other` lies counter-clockwise of `self`, which is how every
    /// orientation question here is answered: which side of a curve a pick is
    /// on, which way a corner turns, whether a loop runs clockwise.
    pub fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
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

    /// This vector turned a quarter turn counter-clockwise.
    pub fn perpendicular(self) -> Self {
        Self::new(-self.y, self.x)
    }

    /// The angle from the positive X axis, in `-π..=π`.
    pub fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }

    /// A fraction `t` of the way to `other`.
    pub fn lerp(self, other: Self, t: f64) -> Self {
        self + (other - self) * t
    }

    /// How far this point is from the segment between `start` and `end`.
    ///
    /// The segment, not the line through it: a point past either end measures
    /// to that end. A segment of no length measures to the point it collapsed
    /// to, which keeps a degenerate input from producing a division by
    /// nothing.
    pub fn distance_to_segment(self, start: Self, end: Self) -> f64 {
        let along = end - start;
        let squared = along.length_squared();
        if squared < 1e-24 {
            return self.distance(start);
        }
        let t = ((self - start).dot(along) / squared).clamp(0.0, 1.0);
        self.distance(start + along * t)
    }
}

impl From<[f64; 2]> for Vec2 {
    fn from(value: [f64; 2]) -> Self {
        Self::new(value[0], value[1])
    }
}

impl From<Vec2> for [f64; 2] {
    fn from(value: Vec2) -> Self {
        value.to_array()
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f64> for Vec2 {
    type Output = Self;
    fn mul(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }
}

impl Div<f64> for Vec2 {
    type Output = Self;
    fn div(self, divisor: f64) -> Self {
        Self::new(self.x / divisor, self.y / divisor)
    }
}

impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn arrays_convert_both_ways_unchanged() {
        let array = [3.0, -4.0];
        let vector = Vec2::from(array);
        assert_eq!(vector.x, 3.0);
        assert_eq!(vector.y, -4.0);
        assert_eq!(<[f64; 2]>::from(vector), array);
    }

    #[test]
    fn arithmetic_is_componentwise() {
        let a = Vec2::new(1.0, 2.0);
        let b = Vec2::new(10.0, 20.0);
        assert_eq!(a + b, Vec2::new(11.0, 22.0));
        assert_eq!(b - a, Vec2::new(9.0, 18.0));
        assert_eq!(a * 3.0, Vec2::new(3.0, 6.0));
        assert_eq!(b / 2.0, Vec2::new(5.0, 10.0));
        assert_eq!(-a, Vec2::new(-1.0, -2.0));
    }

    #[test]
    fn the_dot_product_measures_alignment() {
        let along = Vec2::new(1.0, 0.0);
        assert_eq!(along.dot(Vec2::new(1.0, 0.0)), 1.0);
        assert_eq!(along.dot(Vec2::new(0.0, 1.0)), 0.0);
        assert_eq!(along.dot(Vec2::new(-1.0, 0.0)), -1.0);
    }

    #[test]
    fn the_cross_product_is_a_signed_scalar() {
        let along = Vec2::new(1.0, 0.0);
        // Counter-clockwise is positive, which is the sign every orientation
        // question in this crate reads.
        assert!(along.cross(Vec2::new(0.0, 1.0)) > 0.0);
        assert!(along.cross(Vec2::new(0.0, -1.0)) < 0.0);
        assert_eq!(along.cross(Vec2::new(2.0, 0.0)), 0.0, "parallel");
    }

    #[test]
    fn the_cross_product_is_twice_the_triangle_area() {
        let a = Vec2::new(4.0, 0.0);
        let b = Vec2::new(0.0, 3.0);
        assert_eq!(a.cross(b), 12.0);
    }

    #[test]
    fn length_and_distance_agree() {
        let a = Vec2::new(1.0, 1.0);
        let b = Vec2::new(4.0, 5.0);
        assert_eq!((b - a).length(), 5.0);
        assert_eq!(a.distance(b), 5.0);
        assert_eq!(a.distance_squared(b), 25.0);
    }

    #[test]
    fn normalising_gives_a_unit_vector() {
        let unit = Vec2::new(3.0, 4.0).normalize().unwrap();
        assert!((unit.length() - 1.0).abs() < 1e-15);
        assert!((unit.x - 0.6).abs() < 1e-15);
    }

    #[test]
    fn a_vector_with_no_direction_refuses_to_normalise() {
        assert!(Vec2::ZERO.normalize().is_none());
    }

    #[test]
    fn the_perpendicular_turns_a_quarter_counter_clockwise() {
        let turned = Vec2::new(1.0, 0.0).perpendicular();
        assert_eq!(turned, Vec2::new(0.0, 1.0));
        // Perpendicular to the original, and the turn is the positive one.
        assert_eq!(Vec2::new(1.0, 0.0).dot(turned), 0.0);
        assert!(Vec2::new(1.0, 0.0).cross(turned) > 0.0);
    }

    #[test]
    fn the_angle_is_measured_from_the_x_axis() {
        assert_eq!(Vec2::new(1.0, 0.0).angle(), 0.0);
        assert!((Vec2::new(0.0, 1.0).angle() - FRAC_PI_2).abs() < 1e-15);
        assert!(Vec2::new(0.0, -1.0).angle() < 0.0);
    }

    #[test]
    fn lerp_hits_both_ends_and_the_middle() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(10.0, 20.0);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
        assert_eq!(a.lerp(b, 0.5), Vec2::new(5.0, 10.0));
    }

    #[test]
    fn the_distance_to_a_segment_measures_to_the_segment_not_its_line() {
        let start = Vec2::new(0.0, 0.0);
        let end = Vec2::new(10.0, 0.0);
        // Beside it: the perpendicular.
        assert_eq!(Vec2::new(4.0, 3.0).distance_to_segment(start, end), 3.0);
        // Past the end: to the end, not to the line, which would say 3.
        assert_eq!(Vec2::new(14.0, 3.0).distance_to_segment(start, end), 5.0);
        assert_eq!(Vec2::new(-4.0, 3.0).distance_to_segment(start, end), 5.0);
    }

    #[test]
    fn a_collapsed_segment_measures_to_the_point_it_became() {
        let point = Vec2::new(3.0, 4.0);
        assert_eq!(Vec2::ZERO.distance_to_segment(point, point), 5.0);
    }

    #[test]
    fn survey_coordinates_subtract_without_losing_the_difference() {
        // The conditioning case: two points a millimetre apart at a UTM
        // origin. Differencing them is exact; it is only what comes after that
        // needs a local frame.
        let a = Vec2::new(512_345.678, 4_512_345.678);
        let b = a + Vec2::new(0.001, 0.0);
        assert!((a.distance(b) - 0.001).abs() < 1e-9);
    }
}
