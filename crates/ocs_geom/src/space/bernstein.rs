//! Rounded interval arithmetic and tensor-product Bernstein polynomials: the
//! exact algebra certificates bound.
//!
//! Adapted from Solidcore (NVIDIA, Apache-2.0): `blend/fillet_section/
//! interval.rs` (`Ivl`) and `offset/construction/bernstein.rs`
//! (`Bernstein2`). An [`Interval`] widens every result outward past its
//! rounding, so it holds the exact result for any numbers in its operands; a
//! [`Bernstein2`] lies within the hull of its coefficients on the unit
//! square (Farouki & Rajan 1987), and subdividing it tightens that bound.

use super::nurbs::NurbsSurface3;
use std::ops::{Add, Div, Mul, Sub};
use std::sync::OnceLock;

/// Moves `x` outward by twice an operation's rounding error.
fn widening(x: f64) -> f64 {
    x.abs() * 2.3e-16 + 1.0e-300
}

/// A closed interval of reals, rounded outward. An undefined result — a
/// division by an interval holding zero, a NaN — is the whole line, which no
/// test of a sign passes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    pub lo: f64,
    pub hi: f64,
}

impl From<f64> for Interval {
    fn from(x: f64) -> Self {
        Self { lo: x, hi: x }
    }
}

impl Interval {
    const WHOLE: Self = Self { lo: f64::NEG_INFINITY, hi: f64::INFINITY };

    /// `[lo, hi]` widened by their rounding; the whole line if not ordered.
    pub fn new(lo: f64, hi: f64) -> Self {
        if lo <= hi {
            Self { lo: lo - widening(lo), hi: hi + widening(hi) }
        } else {
            Self::WHOLE
        }
    }

    /// Whether every number in it is positive.
    pub fn is_positive(self) -> bool {
        self.lo > 0.0
    }

    /// Whether every number in it is negative.
    pub fn is_negative(self) -> bool {
        self.hi < 0.0
    }

    /// The sign every number in it has, if they all have one.
    pub fn sign(self) -> Option<f64> {
        if self.is_positive() {
            Some(1.0)
        } else if self.is_negative() {
            Some(-1.0)
        } else {
            None
        }
    }

    /// The largest magnitude in it.
    pub fn mag(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }

    /// The smallest magnitude in it: zero where it holds zero.
    pub fn mig(self) -> f64 {
        if self.lo > 0.0 {
            self.lo
        } else if self.hi < 0.0 {
            -self.hi
        } else {
            0.0
        }
    }

    pub fn mid(self) -> f64 {
        0.5 * (self.lo + self.hi)
    }

    /// The smallest interval holding both.
    pub fn hull(self, other: Self) -> Self {
        Self { lo: self.lo.min(other.lo), hi: self.hi.max(other.hi) }
    }

    /// The square root of its non-negative part.
    pub fn sqrt(self) -> Self {
        if self.hi < 0.0 || self.lo.is_nan() || self.hi.is_nan() {
            return Self::WHOLE;
        }
        Self::new(self.lo.max(0.0).sqrt(), self.hi.sqrt())
    }

    /// Its square, which unlike `x * x` never dips below zero.
    pub fn square(self) -> Self {
        let (a, b) = (self.lo * self.lo, self.hi * self.hi);
        if self.lo <= 0.0 && self.hi >= 0.0 {
            Self::new(0.0, a.max(b))
        } else {
            Self::new(a.min(b), a.max(b))
        }
    }
}

impl Add for Interval {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.lo + other.lo, self.hi + other.hi)
    }
}

impl Sub for Interval {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.lo - other.hi, self.hi - other.lo)
    }
}

impl Mul for Interval {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        let products = [self.lo * other.lo, self.lo * other.hi, self.hi * other.lo, self.hi * other.hi];
        if products.iter().any(|product| product.is_nan()) {
            return Self::WHOLE;
        }
        let lo = products.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = products.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Self::new(lo, hi)
    }
}

impl Div for Interval {
    type Output = Self;
    fn div(self, other: Self) -> Self {
        if other.lo <= 0.0 && other.hi >= 0.0 {
            return Self::WHOLE;
        }
        self * Self::new(1.0 / other.hi, 1.0 / other.lo)
    }
}

/// The largest degree in either direction a polynomial may reach.
pub const MAX_DEGREE: usize = 96;

/// `C(n, k)` for `n <= 2 MAX_DEGREE`.
fn binomial(n: usize, k: usize) -> f64 {
    static ROWS: OnceLock<Vec<Vec<f64>>> = OnceLock::new();
    let rows = ROWS.get_or_init(|| {
        let mut rows: Vec<Vec<f64>> = vec![vec![1.0]];
        for n in 1..=2 * MAX_DEGREE {
            let previous = &rows[n - 1];
            let row = (0..=n)
                .map(|k| if k == 0 || k == n { 1.0 } else { previous[k - 1] + previous[k] })
                .collect();
            rows.push(row);
        }
        rows
    });
    rows.get(n).and_then(|row| row.get(k)).copied().unwrap_or(0.0)
}

/// `Σ c[i][j] B_i^du(s) B_j^dv(t)` on the unit square.
#[derive(Debug, Clone, PartialEq)]
pub struct Bernstein2 {
    du: usize,
    dv: usize,
    /// `c[i * (dv + 1) + j]`.
    c: Vec<f64>,
}

impl Bernstein2 {
    /// `None` for a wrong count of coefficients or a degree past
    /// [`MAX_DEGREE`].
    pub fn new(du: usize, dv: usize, c: Vec<f64>) -> Option<Self> {
        (du <= MAX_DEGREE && dv <= MAX_DEGREE && c.len() == (du + 1) * (dv + 1)).then_some(Self { du, dv, c })
    }

    pub fn constant(x: f64) -> Self {
        Self { du: 0, dv: 0, c: vec![x] }
    }

    pub fn degrees(&self) -> (usize, usize) {
        (self.du, self.dv)
    }

    pub fn coefficients(&self) -> &[f64] {
        &self.c
    }

    /// A lower bound of the polynomial on the square.
    pub fn min_coefficient(&self) -> f64 {
        self.c.iter().copied().fold(f64::INFINITY, f64::min)
    }

    /// An upper bound of the polynomial on the square.
    pub fn max_coefficient(&self) -> f64 {
        self.c.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    }

    /// A bound of its magnitude on the square.
    pub fn max_abs_coefficient(&self) -> f64 {
        self.c.iter().fold(0.0, |most, x| most.max(x.abs()))
    }

    pub fn scale(&self, x: f64) -> Self {
        Self { du: self.du, dv: self.dv, c: self.c.iter().map(|c| c * x).collect() }
    }

    /// The product, of degrees the sums of theirs.
    pub fn mul(&self, other: &Self) -> Option<Self> {
        let (du, dv) = (self.du + other.du, self.dv + other.dv);
        if du > MAX_DEGREE || dv > MAX_DEGREE {
            return None;
        }
        let mut out = vec![0.0; (du + 1) * (dv + 1)];
        for ia in 0..=self.du {
            for ja in 0..=self.dv {
                let x = self.c[ia * (self.dv + 1) + ja] * binomial(self.du, ia) * binomial(self.dv, ja);
                for ib in 0..=other.du {
                    for jb in 0..=other.dv {
                        let y = other.c[ib * (other.dv + 1) + jb] * binomial(other.du, ib) * binomial(other.dv, jb);
                        out[(ia + ib) * (dv + 1) + ja + jb] += x * y;
                    }
                }
            }
        }
        for i in 0..=du {
            for j in 0..=dv {
                out[i * (dv + 1) + j] /= binomial(du, i) * binomial(dv, j);
            }
        }
        Self::new(du, dv, out)
    }

    /// The same polynomial in degrees at least its own.
    pub fn elevate(&self, du: usize, dv: usize) -> Option<Self> {
        if du < self.du || dv < self.dv {
            return None;
        }
        if du == self.du && dv == self.dv {
            return Some(self.clone());
        }
        let ones = Self::new(du - self.du, dv - self.dv, vec![1.0; (du - self.du + 1) * (dv - self.dv + 1)])?;
        self.mul(&ones)
    }

    /// `self + x other`, in the larger degrees.
    pub fn add_scaled(&self, other: &Self, x: f64) -> Option<Self> {
        let (du, dv) = (self.du.max(other.du), self.dv.max(other.dv));
        let (a, b) = (self.elevate(du, dv)?, other.elevate(du, dv)?);
        Self::new(du, dv, a.c.iter().zip(&b.c).map(|(p, q)| p + x * q).collect())
    }

    pub fn add(&self, other: &Self) -> Option<Self> {
        self.add_scaled(other, 1.0)
    }

    pub fn sub(&self, other: &Self) -> Option<Self> {
        self.add_scaled(other, -1.0)
    }

    /// `∂/∂s`, on the unit square.
    pub fn deriv_u(&self) -> Self {
        if self.du == 0 {
            return Self { du: 0, dv: self.dv, c: vec![0.0; self.dv + 1] };
        }
        let width = self.dv + 1;
        let c = (0..self.du)
            .flat_map(|i| (0..width).map(move |j| (i, j)))
            .map(|(i, j)| self.du as f64 * (self.c[(i + 1) * width + j] - self.c[i * width + j]))
            .collect();
        Self { du: self.du - 1, dv: self.dv, c }
    }

    /// `∂/∂t`, on the unit square.
    pub fn deriv_v(&self) -> Self {
        self.transpose().deriv_u().transpose()
    }

    fn transpose(&self) -> Self {
        let mut c = Vec::with_capacity(self.c.len());
        for j in 0..=self.dv {
            c.extend(self.c.iter().skip(j).step_by(self.dv + 1));
        }
        Self { du: self.dv, dv: self.du, c }
    }

    /// The halves split at `t` of the second parameter, each on `[0, 1]`.
    fn split_v(&self, t: f64) -> (Self, Self) {
        let (mut left, mut right) = (Vec::with_capacity(self.c.len()), Vec::with_capacity(self.c.len()));
        for row in self.c.chunks(self.dv + 1) {
            let (l, r) = split_1d(row, t);
            left.extend(l);
            right.extend(r);
        }
        (Self { c: left, ..*self.shape() }, Self { c: right, ..*self.shape() })
    }

    fn shape(&self) -> Box<Self> {
        Box::new(Self { du: self.du, dv: self.dv, c: Vec::new() })
    }

    fn split_u(&self, t: f64) -> (Self, Self) {
        let (left, right) = self.transpose().split_v(t);
        (left.transpose(), right.transpose())
    }

    /// The polynomial on `[u0, u1] × [v0, v1]` of the square, on the unit
    /// square.
    pub fn restrict(&self, u0: f64, u1: f64, v0: f64, v1: f64) -> Self {
        let mut p = self.clone();
        if u0 > 0.0 {
            p = p.split_u(u0).1;
        }
        if u1 < 1.0 {
            p = p.split_u((u1 - u0) / (1.0 - u0)).0;
        }
        if v0 > 0.0 {
            p = p.split_v(v0).1;
        }
        if v1 < 1.0 {
            p = p.split_v((v1 - v0) / (1.0 - v0)).0;
        }
        p
    }

    /// The value at `(s, t)`.
    pub fn eval(&self, s: f64, t: f64) -> f64 {
        let column: Vec<f64> = self.c.chunks(self.dv + 1).map(|row| eval_1d(row, t)).collect();
        eval_1d(&column, s)
    }
}

fn split_1d(c: &[f64], t: f64) -> (Vec<f64>, Vec<f64>) {
    let (mut left, mut right) = (Vec::with_capacity(c.len()), Vec::with_capacity(c.len()));
    let mut current = c.to_vec();
    while let (Some(&first), Some(&last)) = (current.first(), current.last()) {
        left.push(first);
        right.push(last);
        current = current.windows(2).map(|pair| (1.0 - t) * pair[0] + t * pair[1]).collect();
    }
    right.reverse();
    (left, right)
}

fn eval_1d(c: &[f64], t: f64) -> f64 {
    let mut current = c.to_vec();
    while current.len() > 1 {
        current = current.windows(2).map(|pair| (1.0 - t) * pair[0] + t * pair[1]).collect();
    }
    current.first().copied().unwrap_or(0.0)
}

/// A polynomial map of the square into space.
pub type Bernstein3 = [Bernstein2; 3];

pub fn dot(a: &Bernstein3, b: &Bernstein3) -> Option<Bernstein2> {
    a[0].mul(&b[0])?.add(&a[1].mul(&b[1])?)?.add(&a[2].mul(&b[2])?)
}

pub fn cross(a: &Bernstein3, b: &Bernstein3) -> Option<Bernstein3> {
    Some([
        a[1].mul(&b[2])?.sub(&a[2].mul(&b[1])?)?,
        a[2].mul(&b[0])?.sub(&a[0].mul(&b[2])?)?,
        a[0].mul(&b[1])?.sub(&a[1].mul(&b[0])?)?,
    ])
}

pub fn sub3(a: &Bernstein3, b: &Bernstein3) -> Option<Bernstein3> {
    Some([a[0].sub(&b[0])?, a[1].sub(&b[1])?, a[2].sub(&b[2])?])
}

pub fn mul3(a: &Bernstein3, s: &Bernstein2) -> Option<Bernstein3> {
    Some([a[0].mul(s)?, a[1].mul(s)?, a[2].mul(s)?])
}

pub fn deriv_u3(a: &Bernstein3) -> Bernstein3 {
    [a[0].deriv_u(), a[1].deriv_u(), a[2].deriv_u()]
}

pub fn deriv_v3(a: &Bernstein3) -> Bernstein3 {
    [a[0].deriv_v(), a[1].deriv_v(), a[2].deriv_v()]
}

pub fn restrict3(a: &Bernstein3, u0: f64, u1: f64, v0: f64, v1: f64) -> Bernstein3 {
    [a[0].restrict(u0, u1, v0, v1), a[1].restrict(u0, u1, v0, v1), a[2].restrict(u0, u1, v0, v1)]
}

pub fn eval3(a: &Bernstein3, s: f64, t: f64) -> [f64; 3] {
    [a[0].eval(s, t), a[1].eval(s, t), a[2].eval(s, t)]
}

/// One Bézier patch of a spline surface over `range = [u0, u1, v0, v1]` of
/// its knot parameters (in its stored, not reversed, `v`): the homogeneous
/// point `P = w S` and the weight `w`, `None` where every weight is one.
#[derive(Debug, Clone)]
pub struct BezierPatch {
    pub range: [f64; 4],
    pub p: Bernstein3,
    pub w: Option<Bernstein2>,
}

/// A spline surface cut into its Bézier patches, row by row along `u`, by
/// inserting every breakpoint to the full multiplicity.
pub fn bezier_patches(surface: &NurbsSurface3) -> Option<Vec<Vec<BezierPatch>>> {
    let (p, q) = surface.degrees();
    let (u_knots, v_knots) = surface.knots();
    let ((u0, u1), (v0, v1)) = surface.domain();
    let net = surface.control_points();
    let weights = surface.weights();
    let rational = weights.iter().flatten().any(|weight| (*weight - 1.0).abs() > 1e-15);
    // Along v: each row of the net to its segments.
    let mut rows: Vec<Vec<Vec<[f64; 4]>>> = Vec::with_capacity(net.len());
    let mut v_breaks = Vec::new();
    for (row, row_weights) in net.iter().zip(weights) {
        let homogeneous: Vec<[f64; 4]> = row
            .iter()
            .zip(row_weights)
            .map(|(point, weight)| [point[0] * weight, point[1] * weight, point[2] * weight, *weight])
            .collect();
        let (segments, breaks) = decompose(q, v_knots, &homogeneous, (v0, v1))?;
        rows.push(segments);
        v_breaks = breaks;
    }
    // Along u: each column of segment points.
    let mut patches = Vec::new();
    let mut u_breaks = Vec::new();
    let segment_count = rows.first()?.len();
    let mut columns: Vec<Vec<Vec<[f64; 4]>>> = vec![Vec::new(); segment_count];
    for segment in 0..segment_count {
        for j in 0..=q {
            let column: Vec<[f64; 4]> = rows.iter().map(|row| row[segment][j]).collect();
            let (pieces, breaks) = decompose(p, u_knots, &column, (u0, u1))?;
            columns[segment].push(pieces.into_iter().flatten().collect());
            u_breaks = breaks;
        }
    }
    for iu in 0..u_breaks.len() - 1 {
        let mut row = Vec::new();
        for iv in 0..v_breaks.len() - 1 {
            let mut coordinates: [Vec<f64>; 4] = Default::default();
            for i in 0..=p {
                for j in 0..=q {
                    let point = columns[iv][j][iu * (p + 1) + i];
                    for axis in 0..4 {
                        coordinates[axis].push(point[axis]);
                    }
                }
            }
            let [x, y, z, w] = coordinates;
            row.push(BezierPatch {
                range: [u_breaks[iu], u_breaks[iu + 1], v_breaks[iv], v_breaks[iv + 1]],
                p: [Bernstein2::new(p, q, x)?, Bernstein2::new(p, q, y)?, Bernstein2::new(p, q, z)?],
                w: if rational { Some(Bernstein2::new(p, q, w)?) } else { None },
            });
        }
        patches.push(row);
    }
    Some(patches)
}

/// A curve's homogeneous control points cut into Bézier segments over
/// `domain`, and the breakpoints between them (the domain's ends included).
fn decompose(
    degree: usize,
    knots: &[f64],
    points: &[[f64; 4]],
    (start, end): (f64, f64),
) -> Option<(Vec<Vec<[f64; 4]>>, Vec<f64>)> {
    let mut knots = knots.to_vec();
    let mut points = points.to_vec();
    let mut breaks: Vec<f64> = knots.iter().copied().filter(|knot| *knot >= start && *knot <= end).collect();
    breaks.dedup();
    if breaks.first() != Some(&start) || breaks.last() != Some(&end) || breaks.len() < 2 {
        return None;
    }
    for &at in &breaks {
        loop {
            let count = knots.iter().filter(|knot| **knot == at).count();
            if count >= degree {
                break;
            }
            // The span `k` with knots[k] <= at < knots[k + 1], within the
            // points' reach.
            let k = (degree..points.len()).rev().find(|k| knots[*k] <= at)?;
            let mut inserted = Vec::with_capacity(points.len() + 1);
            inserted.extend_from_slice(&points[..=k - degree]);
            for i in k - degree + 1..=k {
                let width = knots[i + degree] - knots[i];
                let alpha = if width == 0.0 { 0.0 } else { (at - knots[i]) / width };
                inserted.push(std::array::from_fn(|axis| points[i - 1][axis] * (1.0 - alpha) + points[i][axis] * alpha));
            }
            inserted.extend_from_slice(&points[k..]);
            knots.insert(k + 1, at);
            points = inserted;
        }
    }
    let mut segments = Vec::with_capacity(breaks.len() - 1);
    for pair in breaks.windows(2) {
        // The last span starting at the segment's start.
        let k = (degree..points.len()).rev().find(|k| knots[*k] <= pair[0] && knots[*k + 1] > pair[0])?;
        segments.push(points[k - degree..=k].to_vec());
    }
    Some((segments, breaks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_hold_the_exact_result() {
        let third = Interval::from(1.0) / Interval::from(3.0);
        assert!(third.lo < 1.0 / 3.0 && third.hi > 1.0 / 3.0);
        let x = Interval::new(-1.0, 2.0);
        assert_eq!(x.square().lo, 0.0 - widening(0.0));
        assert!((x * x).lo < -1.9);
        assert!((Interval::from(1.0) / x).lo == f64::NEG_INFINITY);
        assert_eq!((Interval::from(2.0) - Interval::from(1.0)).sign(), Some(1.0));
    }

    #[test]
    fn bernstein_products_and_derivatives_evaluate_as_the_polynomials() {
        // s(1 - s) t and 1 + s t.
        let a = Bernstein2::new(2, 1, vec![0.0, 0.0, 0.0, 0.5, 0.0, 0.0]).unwrap();
        let b = Bernstein2::new(1, 1, vec![1.0, 1.0, 1.0, 2.0]).unwrap();
        let product = a.mul(&b).unwrap();
        let (s, t) = (0.3, 0.7);
        assert!((product.eval(s, t) - s * (1.0 - s) * t * (1.0 + s * t)).abs() < 1e-14);
        assert!((a.deriv_u().eval(s, t) - (1.0 - 2.0 * s) * t).abs() < 1e-14);
        assert!((a.deriv_v().eval(s, t) - s * (1.0 - s)).abs() < 1e-14);
        let piece = product.restrict(0.2, 0.6, 0.5, 1.0);
        assert!((piece.eval(0.25, 0.4) - product.eval(0.3, 0.7)).abs() < 1e-14);
        assert!(piece.min_coefficient() <= piece.eval(0.5, 0.5) && piece.eval(0.5, 0.5) <= piece.max_coefficient());
    }

    #[test]
    fn a_spline_surface_cuts_into_patches_that_trace_it() {
        let net: Vec<Vec<[f64; 3]>> = (0..5)
            .map(|i| (0..4).map(|j| [i as f64, j as f64, ((i * 7 + j * 3) % 5) as f64 * 0.3]).collect())
            .collect();
        let weights: Vec<Vec<f64>> = (0..5).map(|i| (0..4).map(|j| 1.0 + 0.1 * ((i + j) % 3) as f64).collect()).collect();
        let surface = NurbsSurface3::new(
            3,
            2,
            net,
            vec![0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            Some(weights),
        )
        .unwrap();
        let patches = bezier_patches(&surface).unwrap();
        assert_eq!((patches.len(), patches[0].len()), (2, 2));
        for row in &patches {
            for patch in row {
                let [u0, u1, v0, v1] = patch.range;
                let (s, t) = (0.3, 0.6);
                let p = eval3(&patch.p, s, t);
                let w = patch.w.as_ref().unwrap().eval(s, t);
                let expected = surface.point_at_knot(u0 + s * (u1 - u0), v0 + t * (v1 - v0));
                for axis in 0..3 {
                    assert!((p[axis] / w - expected[axis]).abs() < 1e-12);
                }
            }
        }
    }
}
