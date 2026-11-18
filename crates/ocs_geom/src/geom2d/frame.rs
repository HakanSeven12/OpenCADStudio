//! Local frames: the coordinate policy every operation in this crate obeys.
//!
//! # Why this is an invariant
//!
//! Drawings routinely live at survey coordinates — UTM eastings around
//! 5.0e5, northings around 4.5e6, state-plane values larger still. `f64`
//! carries roughly 15–16 significant decimal digits. An origin at 1.2e6
//! spends seven of them before an operation has done anything, and the
//! remainder is what a subtraction of two nearby points has left to work
//! with.
//!
//! The failures that follow do not look like precision problems, which is
//! what makes them expensive:
//!
//! - circle and arc fits stop converging and silently degrade to polylines
//! - repeated stepping along a curve (linetype patterns, tessellation)
//!   accumulates until an intermediate overflows to infinity
//! - point-in-polygon flips sign near an edge, so fills bleed or vanish
//!
//! Patching each site as it surfaces does not converge, because the next
//! operation added reintroduces it. Lifting at the boundary does.
//!
//! # Contract
//!
//! Every public operation:
//!
//! 1. builds a [`Frame`] from its inputs,
//! 2. calls [`Frame::lift`] on each point before any arithmetic,
//! 3. calls [`Frame::lower`] on every point it returns.
//!
//! Intermediate values never leave local space. A `Frame` is cheap — an
//! origin and nothing else — so an operation that recurses should thread the
//! frame down rather than rebuild it per level.

/// A translation-only local frame.
///
/// Rotation and scale are deliberately absent: they buy no conditioning and
/// would force every caller to reason about orientation. Translation alone
/// recovers the significant digits that a large origin consumes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    origin: [f64; 3],
}

impl Frame {
    /// A frame anchored at `origin`.
    pub const fn at(origin: [f64; 3]) -> Self {
        Self { origin }
    }

    /// The identity frame. Correct only for geometry already near zero;
    /// prefer [`Frame::around`] when the extent is known.
    pub const fn identity() -> Self {
        Self::at([0.0, 0.0, 0.0])
    }

    /// A frame anchored at the midpoint of the bounding box of `points`.
    ///
    /// The midpoint rather than the first point: it bounds the magnitude of
    /// every lifted coordinate by half the extent, whereas anchoring on an
    /// arbitrary member leaves the far side as large as the whole span.
    ///
    /// Returns [`Frame::identity`] for an empty input, which is vacuously
    /// correct — there is nothing to condition.
    pub fn around<'a, I>(points: I) -> Self
    where
        I: IntoIterator<Item = &'a [f64; 3]>,
    {
        let mut iter = points.into_iter();
        let Some(first) = iter.next() else {
            return Self::identity();
        };
        let (mut min, mut max) = (*first, *first);
        for point in iter {
            for axis in 0..3 {
                if point[axis] < min[axis] {
                    min[axis] = point[axis];
                }
                if point[axis] > max[axis] {
                    max[axis] = point[axis];
                }
            }
        }
        Self::at([
            midpoint(min[0], max[0]),
            midpoint(min[1], max[1]),
            midpoint(min[2], max[2]),
        ])
    }

    /// This frame's anchor, in world coordinates.
    pub const fn origin(&self) -> [f64; 3] {
        self.origin
    }

    /// World coordinates in, local coordinates out.
    pub fn lift(&self, point: [f64; 3]) -> [f64; 3] {
        [
            point[0] - self.origin[0],
            point[1] - self.origin[1],
            point[2] - self.origin[2],
        ]
    }

    /// Local coordinates in, world coordinates out.
    pub fn lower(&self, point: [f64; 3]) -> [f64; 3] {
        [
            point[0] + self.origin[0],
            point[1] + self.origin[1],
            point[2] + self.origin[2],
        ]
    }

    /// [`Frame::lift`] for planar work, discarding Z.
    pub fn lift_2d(&self, point: [f64; 2]) -> [f64; 2] {
        [point[0] - self.origin[0], point[1] - self.origin[1]]
    }

    /// [`Frame::lower`] for planar work.
    pub fn lower_2d(&self, point: [f64; 2]) -> [f64; 2] {
        [point[0] + self.origin[0], point[1] + self.origin[1]]
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::identity()
    }
}

/// Averages without forming `a + b`, which would overflow for coordinates
/// near the top of the range and lose a bit everywhere else.
fn midpoint(a: f64, b: f64) -> f64 {
    a + (b - a) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lift_then_lower_returns_the_input() {
        let frame = Frame::at([512_345.678, 4_512_345.678, 91.5]);
        let point = [512_400.5, 4_512_300.25, 100.0];
        let round_tripped = frame.lower(frame.lift(point));
        assert_eq!(round_tripped, point);
    }

    #[test]
    fn around_anchors_on_the_bounding_box_midpoint() {
        let points = [
            [500_000.0, 4_500_000.0, 0.0],
            [500_100.0, 4_500_200.0, 10.0],
        ];
        let frame = Frame::around(points.iter());
        assert_eq!(frame.origin(), [500_050.0, 4_500_100.0, 5.0]);
    }

    #[test]
    fn around_bounds_lifted_magnitude_by_half_the_extent() {
        let points = [
            [500_000.0, 4_500_000.0, 0.0],
            [500_100.0, 4_500_200.0, 10.0],
        ];
        let frame = Frame::around(points.iter());
        for point in points {
            let lifted = frame.lift(point);
            assert!(lifted[0].abs() <= 50.0);
            assert!(lifted[1].abs() <= 100.0);
            assert!(lifted[2].abs() <= 5.0);
        }
    }

    /// Distance to the next representable `f64` above `x`.
    fn spacing_at(x: f64) -> f64 {
        f64::from_bits(x.to_bits() + 1) - x
    }

    #[test]
    fn lifting_restores_the_resolution_later_operations_work_at() {
        // Representable values are spaced by roughly `EPSILON * magnitude`.
        // At a survey origin that spacing is coarser than a nanometre, and it
        // is the floor on every intermediate an operation forms. Lifted into
        // a local frame the same arithmetic has four more decimal orders to
        // give away before it reaches that floor.
        let world = spacing_at(4_500_000.0);
        let local = spacing_at(100.0);

        assert!(world > 1e-10, "world spacing was {world}");
        assert!(local < 1e-13, "local spacing was {local}");
        assert!(world / local > 1e3);
    }

    #[test]
    fn empty_input_yields_the_identity_frame() {
        let none: [[f64; 3]; 0] = [];
        assert_eq!(Frame::around(none.iter()), Frame::identity());
    }
}
