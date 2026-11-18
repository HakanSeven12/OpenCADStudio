//! A spline surface's offset, certified to a tolerance, and the proof that
//! it does not fold.
//!
//! Adapted from Solidcore's `offset/construction/sheets.rs` (NVIDIA,
//! Apache-2.0). The offset `O = S + d n` of a (rational) spline `S` is
//! approximated by a bicubic `G` interpolating exact offset points at the
//! Greville abscissae of its knots — `S`'s breakpoints, at the multiplicity
//! that keeps `G` as smooth as the normal, plus knots inserted where the
//! bound is too large. `G` is certified (Elber & Cohen 1991) on every Bézier
//! patch of the common refinement, from `S = P / w`, the homogeneous
//! tangents `A_u = P_u w − P w_u`, `A_v`, and `D = G w − P`, all exact
//! Bernstein polynomials:
//!
//! * `e = |G − S|² − d²`, bounded by `D·D − d² w²` over `w²`;
//! * the tangential part `τ` of `G − S`, `|τ|² = T / (|A_u × A_v|² w²)`;
//! * `G − S` on the side `d` names: `sign(d) D·(A_u × A_v) > 0`;
//!
//! and then `|G − O| ≤ |τ| + (|e| + |τ|²) / |d|` everywhere.
//!
//! The offset is regular where `det(I − d W) > 0` (`W` the shape operator),
//! which multiplied out is `Q = X'⁴ − d X' H' + d² w² K' > 0` on the reduced
//! normal `A_u × A_v = w N'`: bounded patch by patch and subdivided until
//! certified, or until a patch's middle folds. A fold is refused naming the
//! largest wall that works, the smallest radius of curvature on the
//! displaced side.

use ocs_geom::space::bernstein::{
    bezier_patches, cross, deriv_u3, deriv_v3, dot, eval3, mul3, restrict3, sub3, Bernstein2, Bernstein3,
    BezierPatch,
};
use ocs_geom::space::{NurbsSurface3, Vec3};

/// The most refinement rounds, and knots inserted in one direction.
const MAX_ROUNDS: usize = 14;
const MAX_SPANS: usize = 2048;
/// The most patches the regularity test subdivides into.
const MAX_REGULARITY_PATCHES: usize = 20_000;
const DEGREE: usize = 3;

/// Why a surface has no certified offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OffsetRefusal {
    /// The offset folds over itself; `largest` is the largest distance on
    /// that side that does not.
    Folds { largest: f64 },
    /// Neither regular nor a fold could be shown, or the bound would not
    /// come within the tolerance in the budget.
    NotCertified,
    /// A crease inside the surface, a weight that is not positive, a normal
    /// that vanishes.
    Unsupported,
}

/// The offset and the bound on its distance from the exact offset point at
/// every parameter.
#[derive(Debug, Clone)]
pub struct CertifiedOffset {
    pub surface: NurbsSurface3,
    pub bound: f64,
}

/// `surface` offset by `distance` along its normal (`S_u × S_v`, as
/// [`NurbsSurface3::normal_at_knot`] has it), within `tolerance` at every
/// parameter, over the whole domain and in the surface's own parameters.
/// Refused where the offset folds, naming the largest distance that works.
pub fn certified_offset(
    surface: &NurbsSurface3,
    distance: f64,
    tolerance: f64,
) -> Result<CertifiedOffset, OffsetRefusal> {
    if !(distance.is_finite() && distance != 0.0 && tolerance > 0.0) {
        return Err(OffsetRefusal::Unsupported);
    }
    // In the stored parameters a reversed `v` turns the normal round.
    let d = if surface.v_reversed() { -distance } else { distance };
    let patches = bezier_patches(surface).ok_or(OffsetRefusal::Unsupported)?;
    if patches.iter().flatten().any(|patch| patch.w.as_ref().is_some_and(|w| w.min_coefficient() <= 0.0)) {
        return Err(OffsetRefusal::Unsupported);
    }
    match certify_regular(&patches, d) {
        Regularity::Regular => {}
        Regularity::Folds { largest } => return Err(OffsetRefusal::Folds { largest }),
        Regularity::NotCertified => return Err(OffsetRefusal::NotCertified),
    }
    let (p, q) = surface.degrees();
    let (u_knots, v_knots) = surface.knots();
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (u_breaks, u_mults) = breakpoints(u_knots, (u0, u1));
    let (v_breaks, v_mults) = breakpoints(v_knots, (v0, v1));
    let mut extra_u: Vec<f64> = Vec::new();
    let mut extra_v: Vec<f64> = Vec::new();
    for _ in 0..MAX_ROUNDS {
        let ku = offset_knots(&u_breaks, &u_mults, p, &extra_u).ok_or(OffsetRefusal::Unsupported)?;
        let kv = offset_knots(&v_breaks, &v_mults, q, &extra_v).ok_or(OffsetRefusal::Unsupported)?;
        let (gu, gv) = (greville(&ku), greville(&kv));
        let mut grid = Vec::with_capacity(gu.len());
        for &u in &gu {
            let mut row = Vec::with_capacity(gv.len());
            for &v in &gv {
                let (point, normal) = point_and_normal(&patches, u, v).ok_or(OffsetRefusal::Unsupported)?;
                row.push(point + normal * d);
            }
            grid.push(row);
        }
        let offset = interpolate(&grid, &gu, &gv, &ku, &kv).ok_or(OffsetRefusal::Unsupported)?;
        let offset_patches = bezier_patches(&offset).ok_or(OffsetRefusal::Unsupported)?;
        let mut worst = 0.0_f64;
        let (mut split_u, mut split_v) = (Vec::new(), Vec::new());
        for row in &offset_patches {
            for piece in row {
                let [a, b, c, e] = piece.range;
                let (um, vm) = (0.5 * (a + b), 0.5 * (c + e));
                let base = patch_at(&patches, um, vm).ok_or(OffsetRefusal::Unsupported)?;
                let (s0, t0) = local(base.range, a, c);
                let (s1, t1) = local(base.range, b, e);
                let restricted = BezierPatch {
                    range: piece.range,
                    p: restrict3(&base.p, s0, s1, t0, t1),
                    w: base.w.as_ref().map(|w| w.restrict(s0, s1, t0, t1)),
                };
                let forms = ErrorForms::new(&restricted, &piece.p, d).ok_or(OffsetRefusal::Unsupported)?;
                let mut bound = forms.bound(d);
                if bound > tolerance {
                    let (quarters, by_u, by_v) = forms.split_bounds(d);
                    bound = bound.min(quarters);
                    if bound > tolerance {
                        if by_u <= by_v {
                            split_u.push(um);
                        } else {
                            split_v.push(vm);
                        }
                        if !by_u.is_finite() && !by_v.is_finite() {
                            split_u.push(um);
                            split_v.push(vm);
                        }
                    }
                }
                worst = worst.max(bound);
            }
        }
        if worst <= tolerance {
            let offset = offset
                .with_periodicity(surface.periodicity()[0], surface.periodicity()[1])
                .with_v_reversed(surface.v_reversed());
            return Ok(CertifiedOffset { surface: offset, bound: worst });
        }
        for (extra, mut add) in [(&mut extra_u, split_u), (&mut extra_v, split_v)] {
            add.sort_by(f64::total_cmp);
            add.dedup();
            extra.extend(add);
        }
        if extra_u.len() > MAX_SPANS || extra_v.len() > MAX_SPANS {
            break;
        }
    }
    Err(OffsetRefusal::NotCertified)
}

/// The distinct knots across `domain` and the multiplicity of each interior
/// one.
fn breakpoints(knots: &[f64], (start, end): (f64, f64)) -> (Vec<f64>, Vec<usize>) {
    let mut breaks: Vec<f64> = knots.iter().copied().filter(|knot| *knot >= start && *knot <= end).collect();
    breaks.dedup();
    let mults = breaks
        .iter()
        .skip(1)
        .take(breaks.len().saturating_sub(2))
        .map(|at| knots.iter().filter(|knot| *knot == at).count())
        .collect();
    (breaks, mults)
}

/// The offset's knots in one direction: an interior breakpoint of the base's
/// multiplicity `m` in degree `deg` is taken `3 − min(deg − m − 1, 2)` times,
/// so the offset keeps the normal's continuity; a crease (`m ≥ deg`) has no
/// one offset. Inserted knots are simple.
fn offset_knots(breaks: &[f64], mults: &[usize], deg: usize, extra: &[f64]) -> Option<Vec<f64>> {
    let (lo, hi) = (*breaks.first()?, *breaks.last()?);
    let mut interior: Vec<(f64, usize)> = Vec::new();
    for (at, m) in breaks.iter().skip(1).zip(mults) {
        let continuity = deg.checked_sub(m + 1)?;
        interior.push((*at, DEGREE - continuity.min(DEGREE - 1)));
    }
    interior.extend(extra.iter().filter(|at| !breaks.contains(at)).map(|at| (*at, 1)));
    interior.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut knots = vec![lo; DEGREE + 1];
    for (at, m) in interior {
        knots.extend(std::iter::repeat_n(at, m));
    }
    knots.extend(std::iter::repeat_n(hi, DEGREE + 1));
    Some(knots)
}

fn greville(knots: &[f64]) -> Vec<f64> {
    knots.windows(DEGREE + 2).map(|w| w[1..=DEGREE].iter().sum::<f64>() / DEGREE as f64).collect()
}

/// The cubic B-spline basis functions not zero at `t`, and the first one's
/// index.
fn basis(knots: &[f64], count: usize, t: f64) -> (usize, [f64; DEGREE + 1]) {
    let span = ocs_geom::space::spline::span_of(DEGREE, knots, count - 1, t);
    let mut values = [0.0; DEGREE + 1];
    let (mut left, mut right) = ([0.0; DEGREE + 1], [0.0; DEGREE + 1]);
    values[0] = 1.0;
    for j in 1..=DEGREE {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = values[r] / (right[r + 1] + left[j - r]);
            values[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        values[j] = saved;
    }
    (span - DEGREE, values)
}

/// The control points of the cubic on `knots` through `values` at `params`
/// (Greville abscissae: the collocation matrix is banded and totally
/// positive, so elimination needs no pivoting; de Boor, *A Practical Guide
/// to Splines*, XIII).
fn interpolate_1d(knots: &[f64], params: &[f64], values: &[Vec3]) -> Option<Vec<Vec3>> {
    let n = params.len();
    let width = 2 * DEGREE + 1;
    // Row i holds columns i − DEGREE ..= i + DEGREE.
    let mut band = vec![0.0; n * width];
    for (i, &t) in params.iter().enumerate() {
        let (first, values) = basis(knots, n, t);
        for (k, value) in values.iter().enumerate() {
            let column = first + k;
            let offset = (column + DEGREE).checked_sub(i)?;
            if offset >= width {
                return None;
            }
            band[i * width + offset] = *value;
        }
    }
    let mut rhs = values.to_vec();
    let at = |row: usize, column: usize| row * width + column + DEGREE - row;
    for pivot in 0..n {
        let head = band[at(pivot, pivot)];
        if head.abs() < 1e-300 {
            return None;
        }
        for row in pivot + 1..(pivot + DEGREE + 1).min(n) {
            let factor = band[at(row, pivot)] / head;
            if factor == 0.0 {
                continue;
            }
            for column in pivot..(pivot + DEGREE + 1).min(n) {
                band[at(row, column)] -= factor * band[at(pivot, column)];
            }
            rhs[row] = rhs[row] - rhs[pivot] * factor;
        }
    }
    for pivot in (0..n).rev() {
        let mut value = rhs[pivot];
        for column in pivot + 1..(pivot + DEGREE + 1).min(n) {
            value = value - rhs[column] * band[at(pivot, column)];
        }
        rhs[pivot] = value / band[at(pivot, pivot)];
    }
    Some(rhs)
}

fn interpolate(grid: &[Vec<Vec3>], gu: &[f64], gv: &[f64], ku: &[f64], kv: &[f64]) -> Option<NurbsSurface3> {
    let rows: Vec<Vec<Vec3>> = grid.iter().map(|row| interpolate_1d(kv, gv, row)).collect::<Option<_>>()?;
    let mut net = vec![vec![[0.0; 3]; gv.len()]; gu.len()];
    for column in 0..gv.len() {
        let values: Vec<Vec3> = rows.iter().map(|row| row[column]).collect();
        for (i, point) in interpolate_1d(ku, gu, &values)?.into_iter().enumerate() {
            net[i][column] = point.to_array();
        }
    }
    let weights = vec![vec![1.0; gv.len()]; gu.len()];
    NurbsSurface3::new_strict(DEGREE, DEGREE, net, ku.to_vec(), kv.to_vec(), weights)
}

fn local(range: [f64; 4], u: f64, v: f64) -> (f64, f64) {
    let [a, b, c, e] = range;
    (((u - a) / (b - a)).clamp(0.0, 1.0), ((v - c) / (e - c)).clamp(0.0, 1.0))
}

fn patch_at(patches: &[Vec<BezierPatch>], u: f64, v: f64) -> Option<&BezierPatch> {
    let row = patches.iter().position(|row| u <= row[0].range[1]).unwrap_or(patches.len() - 1);
    let row = patches.get(row)?;
    row.iter().find(|patch| v <= patch.range[3]).or(row.last())
}

/// The homogeneous tangents `A_u = P_u w − P w_u`, `A_v`.
fn homogeneous_tangents(patch: &BezierPatch) -> Option<(Bernstein3, Bernstein3)> {
    let (pu, pv) = (deriv_u3(&patch.p), deriv_v3(&patch.p));
    match &patch.w {
        None => Some((pu, pv)),
        Some(w) => Some((
            sub3(&mul3(&pu, w)?, &mul3(&patch.p, &w.deriv_u())?)?,
            sub3(&mul3(&pv, w)?, &mul3(&patch.p, &w.deriv_v())?)?,
        )),
    }
}

/// `A_u × A_v` with its factor `w` divided out:
/// `N' = w P_u × P_v − w_v P_u × P − w_u P × P_v`.
fn reduced_normal(patch: &BezierPatch) -> Option<Bernstein3> {
    let (pu, pv) = (deriv_u3(&patch.p), deriv_v3(&patch.p));
    let n = cross(&pu, &pv)?;
    match &patch.w {
        None => Some(n),
        Some(w) => sub3(
            &sub3(&mul3(&n, w)?, &mul3(&cross(&pu, &patch.p)?, &w.deriv_v())?)?,
            &mul3(&cross(&patch.p, &pv)?, &w.deriv_u())?,
        ),
    }
}

/// The point and the unit normal at `(u, v)`.
fn point_and_normal(patches: &[Vec<BezierPatch>], u: f64, v: f64) -> Option<(Vec3, Vec3)> {
    let patch = patch_at(patches, u, v)?;
    let (s, t) = local(patch.range, u, v);
    let p = Vec3::from(eval3(&patch.p, s, t));
    let pu = Vec3::from(eval3(&deriv_u3(&patch.p), s, t));
    let pv = Vec3::from(eval3(&deriv_v3(&patch.p), s, t));
    let (point, au, av) = match &patch.w {
        None => (p, pu, pv),
        Some(w) => {
            let (wv, wu, wdv) = (w.eval(s, t), w.deriv_u().eval(s, t), w.deriv_v().eval(s, t));
            (p / wv, pu * wv - p * wu, pv * wv - p * wdv)
        }
    };
    Some((point, au.cross(av).normalize()?))
}

/// The polynomials whose coefficients bound the error on one patch.
struct ErrorForms {
    /// `D·D − d² w²`.
    e: Bernstein2,
    /// `T = (A_v·A_v)(D·A_u)² − 2 (A_u·A_v)(D·A_u)(D·A_v) + (A_u·A_u)(D·A_v)²`.
    t: Bernstein2,
    /// `|A_u × A_v|²`.
    x2: Bernstein2,
    /// `sign(d) D·(A_u × A_v)`.
    side: Bernstein2,
    w: Option<Bernstein2>,
}

impl ErrorForms {
    fn new(base: &BezierPatch, offset: &Bernstein3, d: f64) -> Option<Self> {
        let (au, av) = homogeneous_tangents(base)?;
        let dn = match &base.w {
            None => sub3(offset, &base.p)?,
            Some(w) => sub3(&mul3(offset, w)?, &base.p)?,
        };
        let ax = cross(&au, &av)?;
        let dd = dot(&dn, &dn)?;
        let e = match &base.w {
            None => dd.add_scaled(&Bernstein2::constant(1.0), -d * d)?,
            Some(w) => dd.add_scaled(&w.mul(w)?, -d * d)?,
        };
        let (ru, rv) = (dot(&dn, &au)?, dot(&dn, &av)?);
        let t = dot(&av, &av)?
            .mul(&ru.mul(&ru)?)?
            .add_scaled(&dot(&au, &av)?.mul(&ru.mul(&rv)?)?, -2.0)?
            .add(&dot(&au, &au)?.mul(&rv.mul(&rv)?)?)?;
        Some(Self { e, t, x2: dot(&ax, &ax)?, side: dot(&dn, &ax)?.scale(d.signum()), w: base.w.clone() })
    }

    /// The certified bound on this patch: infinite where a denominator or
    /// the side is not certified.
    fn bound(&self, d: f64) -> f64 {
        let w_min = self.w.as_ref().map_or(1.0, Bernstein2::min_coefficient);
        let x2 = self.x2.min_coefficient();
        if !(w_min > 0.0 && x2 > 0.0 && self.side.min_coefficient() > 0.0) {
            return f64::INFINITY;
        }
        let e = self.e.max_abs_coefficient() / (w_min * w_min);
        let tau2 = self.t.max_coefficient().max(0.0) / (x2 * w_min * w_min);
        tau2.sqrt() + (e + tau2) / d.abs()
    }

    fn restrict(&self, u0: f64, u1: f64, v0: f64, v1: f64) -> Self {
        Self {
            e: self.e.restrict(u0, u1, v0, v1),
            t: self.t.restrict(u0, u1, v0, v1),
            x2: self.x2.restrict(u0, u1, v0, v1),
            side: self.side.restrict(u0, u1, v0, v1),
            w: self.w.as_ref().map(|w| w.restrict(u0, u1, v0, v1)),
        }
    }

    /// The bound after quartering, after halving in `u`, and in `v`.
    fn split_bounds(&self, d: f64) -> (f64, f64, f64) {
        let worst = |boxes: &[[f64; 4]]| {
            boxes.iter().map(|[a, b, c, e]| self.restrict(*a, *b, *c, *e).bound(d)).fold(0.0, f64::max)
        };
        (
            worst(&[[0.0, 0.5, 0.0, 0.5], [0.5, 1.0, 0.0, 0.5], [0.0, 0.5, 0.5, 1.0], [0.5, 1.0, 0.5, 1.0]]),
            worst(&[[0.0, 0.5, 0.0, 1.0], [0.5, 1.0, 0.0, 1.0]]),
            worst(&[[0.0, 1.0, 0.0, 0.5], [0.0, 1.0, 0.5, 1.0]]),
        )
    }
}

/// What [`certify_regular`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Regularity {
    /// `(1 − k1 d)(1 − k2 d) > 0` over the whole surface.
    Regular,
    /// The offset folds; `largest` is the largest distance on that side that
    /// does not.
    Folds { largest: f64 },
    /// Neither shown in the budget.
    NotCertified,
}

/// The regularity forms of one patch: `Q = q0 − X q1`, `X = √x2`.
struct RegularityForms {
    /// `X'⁴ + d² w² K'`.
    q0: Bernstein2,
    /// `d H'`.
    q1: Bernstein2,
    /// `X'²`.
    x2: Bernstein2,
}

impl RegularityForms {
    fn new(patch: &BezierPatch, d: f64) -> Option<Self> {
        let (au, av) = homogeneous_tangents(patch)?;
        let ax = reduced_normal(patch)?;
        let x2 = dot(&ax, &ax)?;
        let second = |pdd: Bernstein3, wdd: Option<Bernstein2>| -> Option<Bernstein2> {
            let v = match (&patch.w, wdd) {
                (Some(w), Some(wdd)) => sub3(&mul3(&pdd, w)?, &mul3(&patch.p, &wdd)?)?,
                _ => pdd,
            };
            dot(&v, &ax)
        };
        let w = patch.w.as_ref();
        let l = second(deriv_u3(&deriv_u3(&patch.p)), w.map(|w| w.deriv_u().deriv_u()))?;
        let m = second(deriv_v3(&deriv_u3(&patch.p)), w.map(|w| w.deriv_u().deriv_v()))?;
        let n = second(deriv_v3(&deriv_v3(&patch.p)), w.map(|w| w.deriv_v().deriv_v()))?;
        let (e1, f1, g1) = (dot(&au, &au)?, dot(&au, &av)?, dot(&av, &av)?);
        let h = e1.mul(&n)?.add(&g1.mul(&l)?)?.add_scaled(&f1.mul(&m)?, -2.0)?;
        let k = l.mul(&n)?.sub(&m.mul(&m)?)?;
        let w2 = match w {
            Some(w) => w.mul(w)?,
            None => Bernstein2::constant(1.0),
        };
        let q0 = x2.mul(&x2)?.add_scaled(&w2.mul(&k)?, d * d)?;
        Some(Self { q0, q1: h.scale(d), x2 })
    }

    fn certified(&self) -> bool {
        let x2_min = self.x2.min_coefficient();
        if !(x2_min > 0.0) {
            return false;
        }
        let q1_max = self.q1.max_coefficient();
        let worst = if q1_max > 0.0 { self.x2.max_coefficient().sqrt() * q1_max } else { x2_min.sqrt() * q1_max };
        self.q0.min_coefficient() - worst > 0.0
    }

    /// `Q` at the middle, and the size of its terms.
    fn middle(&self) -> (f64, f64) {
        let x = self.x2.eval(0.5, 0.5).max(0.0).sqrt();
        let (q0, q1) = (self.q0.eval(0.5, 0.5), x * self.q1.eval(0.5, 0.5));
        (q0 - q1, q0.abs() + q1.abs())
    }

    fn quarters(&self) -> [Self; 4] {
        let r = |u0, u1, v0, v1| Self {
            q0: self.q0.restrict(u0, u1, v0, v1),
            q1: self.q1.restrict(u0, u1, v0, v1),
            x2: self.x2.restrict(u0, u1, v0, v1),
        };
        [r(0.0, 0.5, 0.0, 0.5), r(0.5, 1.0, 0.0, 0.5), r(0.0, 0.5, 0.5, 1.0), r(0.5, 1.0, 0.5, 1.0)]
    }
}

/// Whether the offset of the patches by `d` (along their stored normal) is
/// regular.
fn certify_regular(patches: &[Vec<BezierPatch>], d: f64) -> Regularity {
    let mut stack = Vec::new();
    for patch in patches.iter().flatten() {
        match RegularityForms::new(patch, d) {
            Some(forms) => stack.push(forms),
            None => return Regularity::NotCertified,
        }
    }
    let mut examined = 0;
    let mut folds = false;
    while let Some(forms) = stack.pop() {
        examined += 1;
        if forms.certified() {
            continue;
        }
        // ponytail: a fold is taken where Q at a patch's middle is clearly
        // negative in floating point; Solidcore proves it in interval
        // arithmetic on the net's partials, add that if a borderline fold is
        // ever misjudged.
        let (q, size) = forms.middle();
        if q < -1e-9 * size {
            folds = true;
            break;
        }
        if examined > MAX_REGULARITY_PATCHES {
            return Regularity::NotCertified;
        }
        stack.extend(forms.quarters());
    }
    if !folds {
        return Regularity::Regular;
    }
    let largest = largest_wall(patches, d.signum());
    if largest.is_finite() {
        Regularity::Folds { largest }
    } else {
        Regularity::NotCertified
    }
}

/// The principal curvatures, signed along the stored normal, at `(s, t)`.
fn principal_curvatures(patch: &BezierPatch, s: f64, t: f64) -> Option<(f64, f64)> {
    let at = |form: &Bernstein3| Vec3::from(eval3(form, s, t));
    let p = at(&patch.p);
    let (pu, pv) = (at(&deriv_u3(&patch.p)), at(&deriv_v3(&patch.p)));
    let (puu, puv, pvv) = (
        at(&deriv_u3(&deriv_u3(&patch.p))),
        at(&deriv_v3(&deriv_u3(&patch.p))),
        at(&deriv_v3(&deriv_v3(&patch.p))),
    );
    let (w, au, av, l2, m2, n2) = match &patch.w {
        None => (1.0, pu, pv, puu, puv, pvv),
        Some(w) => {
            let value = w.eval(s, t);
            let (wu, wv) = (w.deriv_u().eval(s, t), w.deriv_v().eval(s, t));
            let (wuu, wuv, wvv) = (
                w.deriv_u().deriv_u().eval(s, t),
                w.deriv_u().deriv_v().eval(s, t),
                w.deriv_v().deriv_v().eval(s, t),
            );
            (value, pu * value - p * wu, pv * value - p * wv, puu * value - p * wuu, puv * value - p * wuv, pvv * value - p * wvv)
        }
    };
    let ax = au.cross(av);
    let x = ax.length();
    if !(x > 0.0) {
        return None;
    }
    let (e, f, g) = (au.dot(au), au.dot(av), av.dot(av));
    let (l, m, n) = (l2.dot(ax), m2.dot(ax), n2.dot(ax));
    // λ² X² − λ H + K = 0, κ = w² λ / X.
    let h = e * n + g * l - 2.0 * f * m;
    let k = l * n - m * m;
    let a = x * x;
    let root = (h * h - 4.0 * a * k).max(0.0).sqrt();
    let scale = w * w / x;
    Some((scale * (h - root) / (2.0 * a), scale * (h + root) / (2.0 * a)))
}

/// The radius of the largest curvature towards `side` at `(s, t)`.
fn wall(patch: &BezierPatch, side: f64, s: f64, t: f64) -> f64 {
    match principal_curvatures(patch, s, t) {
        Some((k1, k2)) => {
            let k = (k1 * side).max(k2 * side);
            if k > 0.0 { 1.0 / k } else { f64::INFINITY }
        }
        None => f64::INFINITY,
    }
}

/// The smallest radius of curvature towards `side`: a 9 × 9 grid per patch,
/// then a pattern search about the smallest sample.
fn largest_wall(patches: &[Vec<BezierPatch>], side: f64) -> f64 {
    let mut best = (f64::INFINITY, None, 0.5, 0.5);
    for patch in patches.iter().flatten() {
        for a in 0..=8 {
            for c in 0..=8 {
                let (s, t) = (f64::from(a) / 8.0, f64::from(c) / 8.0);
                let radius = wall(patch, side, s, t);
                if radius < best.0 {
                    best = (radius, Some(patch), s, t);
                }
            }
        }
    }
    let (mut radius, Some(patch), mut s, mut t) = best else {
        return f64::INFINITY;
    };
    let mut step = 1.0 / 16.0;
    for _ in 0..60 {
        let mut moved = false;
        for (ds, dt) in [(step, 0.0), (-step, 0.0), (0.0, step), (0.0, -step)] {
            let (s2, t2) = ((s + ds).clamp(0.0, 1.0), (t + dt).clamp(0.0, 1.0));
            let candidate = wall(patch, side, s2, t2);
            if candidate < radius {
                (radius, s, t, moved) = (candidate, s2, t2, true);
            }
        }
        if !moved {
            step *= 0.5;
        }
    }
    radius
}

/// The largest distance `surface` can be offset by towards the side of
/// `distance`'s sign without folding: the smallest radius of curvature
/// bending that way, infinite where none does.
pub fn largest_offset(surface: &NurbsSurface3, distance: f64) -> Option<f64> {
    let side = if surface.v_reversed() { -distance.signum() } else { distance.signum() };
    Some(largest_wall(&bezier_patches(surface)?, side))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter of a cylinder of radius 2, rational and exact, `v` along
    /// its axis.
    fn quarter_cylinder() -> NurbsSurface3 {
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let net = [[2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]
            .iter()
            .map(|[x, y]| vec![[*x, *y, 0.0], [*x, *y, 3.0]])
            .collect();
        NurbsSurface3::new(
            2,
            1,
            net,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            Some(vec![vec![1.0, 1.0], vec![half, half], vec![1.0, 1.0]]),
        )
        .unwrap()
    }

    #[test]
    fn a_rational_cylinder_offsets_within_its_certified_bound() {
        let surface = quarter_cylinder();
        // Its normal `S_u × S_v` points out: growing by 0.5.
        let offset = certified_offset(&surface, 0.5, 1e-6).unwrap();
        assert!(offset.bound <= 1e-6);
        for i in 0..=10 {
            for j in 0..=4 {
                let (u, v) = (i as f64 / 10.0, j as f64 / 4.0);
                let point = offset.surface.point_at_knot(u, v);
                let radius = point[0].hypot(point[1]);
                assert!((radius - 2.5).abs() <= 1e-6, "{radius}");
                let expected = Vec3::from(surface.point_at_knot(u, v)) * 1.25;
                assert!(Vec3::from(point).distance(Vec3::new(expected.x, expected.y, point[2])) <= 1e-6);
            }
        }
    }

    #[test]
    fn an_inward_offset_past_the_radius_is_refused_naming_the_radius() {
        let surface = quarter_cylinder();
        assert!(certified_offset(&surface, -1.5, 1e-6).is_ok());
        let Err(OffsetRefusal::Folds { largest }) = certified_offset(&surface, -2.5, 1e-6) else {
            panic!("a fold");
        };
        assert!((largest - 2.0).abs() < 1e-6, "{largest}");
        assert!((largest_offset(&surface, -1.0).unwrap() - 2.0).abs() < 1e-6);
        assert!(largest_offset(&surface, 1.0).unwrap().is_infinite());
    }
}
