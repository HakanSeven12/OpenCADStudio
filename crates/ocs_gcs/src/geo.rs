//! Curve geometry: points, the value-with-derivative type, and the curves
//! the solver operates on (line, circle, arc — conics and B-splines are
//! deferred to later port stages).
//!
//! Ported from planegcs's `Geo.h`/`Geo.cpp`. The one structural change from
//! the C++: planegcs's `PushOwnParams`/`ReconstructOnNewPvec` exist only to
//! support its raw-pointer parameter-redirection trick (see `util.rs`); since
//! curves here hold [`ParamId`]s rather than pointers, no such
//! reconstruction is ever needed and those methods have no equivalent.

use crate::util::{ParamId, ParamStore};

/// A 2D point addressed by parameter id, not by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: ParamId,
    pub y: ParamId,
}

impl Point {
    pub fn new(x: ParamId, y: ParamId) -> Self {
        Self { x, y }
    }
}

/// A 2D vector paired with its derivative w.r.t. whichever parameter the
/// caller is currently differentiating for ("derivparam"). Reading a
/// [`Point`] through [`DeriVector2::from_point`] sets `dx`/`dy` to 1 where
/// the point's own x/y parameter *is* derivparam, 0 otherwise — the seed
/// values automatic-differentiation-by-hand starts from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeriVector2 {
    pub x: f64,
    pub dx: f64,
    pub y: f64,
    pub dy: f64,
}

impl DeriVector2 {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, dx: 0.0, y, dy: 0.0 }
    }

    pub fn with_deriv(x: f64, y: f64, dx: f64, dy: f64) -> Self {
        Self { x, dx, y, dy }
    }

    /// Reads a point's value from `store`, seeding the derivative as 1 for
    /// whichever of the point's coordinates equals `derivparam`.
    pub fn from_point(store: &ParamStore, p: Point, derivparam: Option<ParamId>) -> Self {
        Self {
            x: store.get(p.x),
            dx: if derivparam == Some(p.x) { 1.0 } else { 0.0 },
            y: store.get(p.y),
            dy: if derivparam == Some(p.y) { 1.0 } else { 0.0 },
        }
    }

    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Returns `(length, d(length))`.
    pub fn length_deriv(&self) -> (f64, f64) {
        let l = self.length();
        if l == 0.0 {
            return (l, 1.0);
        }
        (l, (self.x * self.dx + self.y * self.dy) / l)
    }

    /// Unlike the other operations here, this returns a new vector rather
    /// than mutating `self` — matching planegcs's own `getNormalized`.
    /// Returns the zero vector (derivative preserved) if `self` is zero.
    pub fn normalized(&self) -> Self {
        let l = self.length();
        if l == 0.0 {
            return Self { x: 0.0, dx: self.dx, y: 0.0, dy: self.dy };
        }
        let mut rtn = Self { x: self.x / l, y: self.y / l, dx: self.dx / l, dy: self.dy / l };
        let dsc = rtn.dx * rtn.x + rtn.dy * rtn.y;
        rtn.dx -= dsc * rtn.x;
        rtn.dy -= dsc * rtn.y;
        rtn
    }

    /// Returns `(scalar product, d(scalar product))`.
    pub fn scalar_prod(&self, v2: &Self) -> (f64, f64) {
        let dprd = self.dx * v2.x + self.x * v2.dx + self.dy * v2.y + self.y * v2.dy;
        (self.x * v2.x + self.y * v2.y, dprd)
    }

    /// Returns `(z of cross product, d(z of cross product))`, treating both
    /// vectors as 3D with a zero z component.
    pub fn cross_prod_z(&self, v2: &Self) -> (f64, f64) {
        let dprd = self.dx * v2.y + self.x * v2.dy - self.dy * v2.x - self.y * v2.dx;
        (self.x * v2.y - self.y * v2.x, dprd)
    }

    pub fn sum(&self, v2: &Self) -> Self {
        Self { x: self.x + v2.x, y: self.y + v2.y, dx: self.dx + v2.dx, dy: self.dy + v2.dy }
    }

    pub fn subtr(&self, v2: &Self) -> Self {
        Self { x: self.x - v2.x, y: self.y - v2.y, dx: self.dx - v2.dx, dy: self.dy - v2.dy }
    }

    pub fn mult(&self, val: f64) -> Self {
        Self { x: self.x * val, y: self.y * val, dx: self.dx * val, dy: self.dy * val }
    }

    /// Multiplies by a scalar that itself has a derivative (`val`, `dval`).
    pub fn mult_d(&self, val: f64, dval: f64) -> Self {
        Self {
            x: self.x * val,
            y: self.y * val,
            dx: self.dx * val + self.x * dval,
            dy: self.dy * val + self.y * dval,
        }
    }

    /// Divides by a scalar that itself has a derivative (`val`, `dval`).
    pub fn div_d(&self, val: f64, dval: f64) -> Self {
        Self {
            x: self.x / val,
            y: self.y / val,
            dx: self.dx / val - self.x * dval / (val * val),
            dy: self.dy / val - self.y * dval / (val * val),
        }
    }

    pub fn rotate90ccw(&self) -> Self {
        Self { x: -self.y, y: self.x, dx: -self.dy, dy: self.dx }
    }

    pub fn rotate90cw(&self) -> Self {
        Self { x: self.y, y: -self.x, dx: self.dy, dy: -self.dx }
    }

    /// Linear combination `self * m1 + v2 * m2`.
    pub fn lin_combi(&self, m1: f64, v2: &Self, m2: f64) -> Self {
        Self {
            x: self.x * m1 + v2.x * m2,
            y: self.y * m1 + v2.y * m2,
            dx: self.dx * m1 + v2.dx * m2,
            dy: self.dy * m1 + v2.dy * m2,
        }
    }
}

/// A curve a constraint can evaluate: a point on it (and its normal) for a
/// given parameter `u`, differentiated w.r.t. `derivparam`.
///
/// Normals point to the left when walking the curve from start to end;
/// circles/arcs are walked counterclockwise, so their normal points inward.
///
/// Object-safe by design: constraints that work with an arbitrary curve type
/// picked at construction time (`CurveValue`, the `AngleViaPoint` family,
/// `Snell` — see [`crate::constraints::curve_generic`]) hold it as a
/// `Rc<dyn Curve>`, mirroring planegcs's own `Curve*` fields in those
/// constraint classes.
pub trait Curve {
    /// Normal at a point `p` that need not lie exactly on the curve (e.g. a
    /// not-yet-solved sketch point being pulled onto it). Default impl
    /// delegates to [`calculate_normal_from_value`](Self::calculate_normal_from_value)
    /// after sampling `p` from the store — what differs per curve type is
    /// only the formula once you *have* the point's value, not how the
    /// point's value is obtained.
    fn calculate_normal(&self, store: &ParamStore, p: Point, derivparam: Option<ParamId>) -> DeriVector2 {
        self.calculate_normal_from_value(store, DeriVector2::from_point(store, p, derivparam), derivparam)
    }

    /// The per-curve-type formula behind [`calculate_normal`](Self::calculate_normal)
    /// and [`calculate_normal_at_param`](Self::calculate_normal_at_param) alike, taking
    /// the point already resolved to a value (with derivative) rather than a
    /// stored [`Point`] — since [`calculate_normal_at_param`](Self::calculate_normal_at_param)
    /// has no `Point` to give it, only a value computed from `u`.
    fn calculate_normal_from_value(&self, store: &ParamStore, p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2;

    /// Normal at the curve's own point-at-parameter `u`, rather than at a
    /// separately stored [`Point`]. Mirrors planegcs's second `CalculateNormal`
    /// overload (`Geo.h`)'s *default* implementation (the one every curve
    /// here uses — only `BSpline` gives it a real override, with a full
    /// de Boor derivation, which this port doesn't implement): the point on
    /// the curve is sampled with `derivparam = None`, always, even when the
    /// call came in with a real `derivparam` — so the point's own *position*
    /// never contributes to the returned derivative, only whatever the
    /// normal formula picks up directly and independently from the curve's
    /// own shape parameters.
    ///
    /// This is a real, documented limitation in upstream planegcs, not a
    /// simplification of this port, and it is broader than just "`u` is
    /// exempt from differentiation": for any curve whose normal formula
    /// itself depends on the point's position (`Circle`/`Arc`, the conics —
    /// their [`calculate_normal_from_value`](Self::calculate_normal_from_value)
    /// reads `p`), differentiating w.r.t. *any* of that curve's own shape
    /// parameters is silently incomplete here too, because the true
    /// position shift such a parameter causes never reaches the frozen `p`.
    /// `Line`'s normal doesn't depend on position at all, so it alone is
    /// unaffected — every parameter's derivative through it is exact.
    fn calculate_normal_at_param(&self, store: &ParamStore, u: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        let p = self.value(store, u, 0.0, None);
        self.calculate_normal_from_value(store, p, derivparam)
    }

    fn value(&self, store: &ParamStore, u: f64, du: f64, derivparam: Option<ParamId>) -> DeriVector2;

    /// This curve's own parameters (its defining points/radii/angles) —
    /// planegcs's `PushOwnParams`. Needed only by constraints that hold a
    /// curve generically (as `Rc<dyn Curve>`) and so can't just list a
    /// concrete struct's fields by hand the way e.g. `Parallel`
    /// (`point_line.rs`) does for its own `Line`s.
    fn own_params(&self) -> Vec<ParamId>;
}

#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub p1: Point,
    pub p2: Point,
}

impl Curve for Line {
    fn calculate_normal_from_value(&self, store: &ParamStore, _p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2 {
        let p1v = DeriVector2::from_point(store, self.p1, derivparam);
        let p2v = DeriVector2::from_point(store, self.p2, derivparam);
        p2v.subtr(&p1v).rotate90ccw()
    }

    fn value(&self, store: &ParamStore, u: f64, du: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        let p1v = DeriVector2::from_point(store, self.p1, derivparam);
        let p2v = DeriVector2::from_point(store, self.p2, derivparam);
        let line_vec = p2v.subtr(&p1v);
        p1v.sum(&line_vec.mult_d(u, du))
    }

    fn own_params(&self) -> Vec<ParamId> {
        vec![self.p1.x, self.p1.y, self.p2.x, self.p2.y]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Circle {
    pub center: Point,
    pub rad: ParamId,
}

impl Curve for Circle {
    fn calculate_normal_from_value(&self, store: &ParamStore, p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2 {
        let cv = DeriVector2::from_point(store, self.center, derivparam);
        cv.subtr(&p)
    }

    fn value(&self, store: &ParamStore, u: f64, du: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        let cv = DeriVector2::from_point(store, self.center, derivparam);
        let r = store.get(self.rad);
        let dr = if derivparam == Some(self.rad) { 1.0 } else { 0.0 };
        let ex = DeriVector2::with_deriv(r, 0.0, dr, 0.0);
        let ey = ex.rotate90ccw();
        let (si, co) = u.sin_cos();
        let dsi = du * co;
        let dco = du * (-si);
        cv.sum(&ex.mult_d(co, dco).sum(&ey.mult_d(si, dsi)))
    }

    fn own_params(&self) -> Vec<ParamId> {
        vec![self.center.x, self.center.y, self.rad]
    }
}

/// Arc inherits Circle's `Value`/`CalculateNormal` unchanged in planegcs (it
/// only adds start/end/angle bookkeeping used by the higher-level "arc
/// rules" constraint, not by the curve evaluation itself) — so here it just
/// wraps a `Circle` and delegates.
#[derive(Debug, Clone, Copy)]
pub struct Arc {
    pub circle: Circle,
    pub start: Point,
    pub end: Point,
    pub start_angle: ParamId,
    pub end_angle: ParamId,
}

impl Curve for Arc {
    fn calculate_normal_from_value(&self, store: &ParamStore, p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2 {
        self.circle.calculate_normal_from_value(store, p, derivparam)
    }

    fn value(&self, store: &ParamStore, u: f64, du: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        self.circle.value(store, u, du, derivparam)
    }

    fn own_params(&self) -> Vec<ParamId> {
        let mut params = self.circle.own_params();
        params.extend([self.start.x, self.start.y, self.end.x, self.end.y, self.start_angle, self.end_angle]);
        params
    }
}

/// An ellipse given by its centre, one focus, and its minor radius (the
/// major radius/axis follow from those three, via [`Ellipse::rad_maj`]).
#[derive(Debug, Clone, Copy)]
pub struct Ellipse {
    pub center: Point,
    pub focus1: Point,
    pub radmin: ParamId,
}

impl Ellipse {
    /// Major radius and its derivative, given already-sampled `center`/`f1`
    /// vectors and the minor radius `b`/`db` — exposed (as in the C++) so
    /// constraint code can reuse `DeriVector2`s it already built rather than
    /// resampling from the store.
    pub fn rad_maj(center: &DeriVector2, f1: &DeriVector2, b: f64, db: f64) -> (f64, f64) {
        let cf = f1.subtr(center);
        let (cf_len, dcf) = cf.length_deriv();
        // A "vector" (b, cf_len) whose own length formula happens to equal
        // the major-radius formula (a² = b² + c²) — reuses `length_deriv`
        // rather than repeating the same sqrt-of-sum-of-squares by hand.
        let hack = DeriVector2::with_deriv(b, cf_len, db, dcf);
        hack.length_deriv()
    }

    /// [`rad_maj`](Self::rad_maj), sampling `center`/`focus1`/`radmin` from
    /// `store` itself — mirrors the C++'s other `getRadMaj(derivparam, ...)`
    /// overload.
    pub fn rad_maj_at(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        let c = DeriVector2::from_point(store, self.center, derivparam);
        let f1 = DeriVector2::from_point(store, self.focus1, derivparam);
        let b = store.get(self.radmin);
        let db = if derivparam == Some(self.radmin) { 1.0 } else { 0.0 };
        Self::rad_maj(&c, &f1, b, db)
    }
}

impl Curve for Ellipse {
    fn calculate_normal_from_value(&self, store: &ParamStore, p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2 {
        let cv = DeriVector2::from_point(store, self.center, derivparam);
        let f1v = DeriVector2::from_point(store, self.focus1, derivparam);

        let f2v = cv.lin_combi(2.0, &f1v, -1.0);
        let pf1 = f1v.subtr(&p);
        let pf2 = f2v.subtr(&p);
        pf1.normalized().sum(&pf2.normalized())
    }

    fn own_params(&self) -> Vec<ParamId> {
        vec![self.center.x, self.center.y, self.focus1.x, self.focus1.y, self.radmin]
    }

    fn value(&self, store: &ParamStore, u: f64, du: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        let c = DeriVector2::from_point(store, self.center, derivparam);
        let f1 = DeriVector2::from_point(store, self.focus1, derivparam);

        let emaj = f1.subtr(&c).normalized();
        let emin = emaj.rotate90ccw();
        let b = store.get(self.radmin);
        let db = if derivparam == Some(self.radmin) { 1.0 } else { 0.0 };
        let (a, da) = Self::rad_maj(&c, &f1, b, db);
        let a_vec = emaj.mult_d(a, da);
        let b_vec = emin.mult_d(b, db);

        let (si, co) = u.sin_cos();
        let dco = -si * du;
        let dsi = co * du;

        a_vec.mult_d(co, dco).sum(&b_vec.mult_d(si, dsi)).sum(&c)
    }
}

/// Inherits `Ellipse`'s `Value`/`CalculateNormal` unchanged, same as `Arc`
/// does for `Circle`.
#[derive(Debug, Clone, Copy)]
pub struct ArcOfEllipse {
    pub ellipse: Ellipse,
    pub start: Point,
    pub end: Point,
    pub start_angle: ParamId,
    pub end_angle: ParamId,
}

impl Curve for ArcOfEllipse {
    fn calculate_normal_from_value(&self, store: &ParamStore, p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2 {
        self.ellipse.calculate_normal_from_value(store, p, derivparam)
    }

    fn value(&self, store: &ParamStore, u: f64, du: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        self.ellipse.value(store, u, du, derivparam)
    }

    fn own_params(&self) -> Vec<ParamId> {
        let mut params = self.ellipse.own_params();
        params.extend([self.start.x, self.start.y, self.end.x, self.end.y, self.start_angle, self.end_angle]);
        params
    }
}

/// A hyperbola given the same way as [`Ellipse`] (centre, one focus, minor
/// radius); `rad_maj` uses `a² = c² - b²` instead of `a² = b² + c²`.
#[derive(Debug, Clone, Copy)]
pub struct Hyperbola {
    pub center: Point,
    pub focus1: Point,
    pub radmin: ParamId,
}

impl Hyperbola {
    pub fn rad_maj(center: &DeriVector2, f1: &DeriVector2, b: f64, db: f64) -> (f64, f64) {
        let cf = f1.subtr(center);
        let (cf_len, dcf) = cf.length_deriv();
        let a = (cf_len * cf_len - b * b).sqrt();
        let da = (dcf * cf_len - db * b) / a;
        (a, da)
    }

    pub fn rad_maj_at(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        let c = DeriVector2::from_point(store, self.center, derivparam);
        let f1 = DeriVector2::from_point(store, self.focus1, derivparam);
        let b = store.get(self.radmin);
        let db = if derivparam == Some(self.radmin) { 1.0 } else { 0.0 };
        Self::rad_maj(&c, &f1, b, db)
    }
}

/// Either kind of "major radius conic" — the closed set planegcs's abstract
/// `MajorRadiusConic` base class stands in for, needed so a constraint like
/// [`EqualMajorAxesConic`](crate::constraints::conic::EqualMajorAxesConic)
/// can compare an ellipse against a hyperbola without knowing which is
/// which.
#[derive(Debug, Clone, Copy)]
pub enum Conic {
    Ellipse(Ellipse),
    Hyperbola(Hyperbola),
}

impl Conic {
    pub fn rad_maj_at(&self, store: &ParamStore, derivparam: Option<ParamId>) -> (f64, f64) {
        match self {
            Conic::Ellipse(e) => e.rad_maj_at(store, derivparam),
            Conic::Hyperbola(h) => h.rad_maj_at(store, derivparam),
        }
    }

    pub fn own_params(&self) -> Vec<ParamId> {
        match self {
            Conic::Ellipse(e) => vec![e.center.x, e.center.y, e.focus1.x, e.focus1.y, e.radmin],
            Conic::Hyperbola(h) => vec![h.center.x, h.center.y, h.focus1.x, h.focus1.y, h.radmin],
        }
    }
}

impl Curve for Hyperbola {
    fn calculate_normal_from_value(&self, store: &ParamStore, p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2 {
        let cv = DeriVector2::from_point(store, self.center, derivparam);
        let f1v = DeriVector2::from_point(store, self.focus1, derivparam);

        let f2v = cv.lin_combi(2.0, &f1v, -1.0);
        // Differs from the ellipse's normal by inverting this vector, as in
        // the C++.
        let pf1 = f1v.subtr(&p).mult(-1.0);
        let pf2 = f2v.subtr(&p);
        pf1.normalized().sum(&pf2.normalized())
    }

    fn own_params(&self) -> Vec<ParamId> {
        vec![self.center.x, self.center.y, self.focus1.x, self.focus1.y, self.radmin]
    }

    fn value(&self, store: &ParamStore, u: f64, du: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        let c = DeriVector2::from_point(store, self.center, derivparam);
        let f1 = DeriVector2::from_point(store, self.focus1, derivparam);

        let emaj = f1.subtr(&c).normalized();
        let emin = emaj.rotate90ccw();
        let b = store.get(self.radmin);
        let db = if derivparam == Some(self.radmin) { 1.0 } else { 0.0 };
        let (a, da) = Self::rad_maj(&c, &f1, b, db);
        let a_vec = emaj.mult_d(a, da);
        let b_vec = emin.mult_d(b, db);

        let co = u.cosh();
        let si = u.sinh();
        let dco = si * du;
        let dsi = co * du;

        a_vec.mult_d(co, dco).sum(&b_vec.mult_d(si, dsi)).sum(&c)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ArcOfHyperbola {
    pub hyperbola: Hyperbola,
    pub start: Point,
    pub end: Point,
    pub start_angle: ParamId,
    pub end_angle: ParamId,
}

impl Curve for ArcOfHyperbola {
    fn calculate_normal_from_value(&self, store: &ParamStore, p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2 {
        self.hyperbola.calculate_normal_from_value(store, p, derivparam)
    }

    fn value(&self, store: &ParamStore, u: f64, du: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        self.hyperbola.value(store, u, du, derivparam)
    }

    fn own_params(&self) -> Vec<ParamId> {
        let mut params = self.hyperbola.own_params();
        params.extend([self.start.x, self.start.y, self.end.x, self.end.y, self.start_angle, self.end_angle]);
        params
    }
}

/// A parabola given by its vertex and one focus.
#[derive(Debug, Clone, Copy)]
pub struct Parabola {
    pub vertex: Point,
    pub focus1: Point,
}

impl Curve for Parabola {
    fn calculate_normal_from_value(&self, store: &ParamStore, p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2 {
        let cv = DeriVector2::from_point(store, self.vertex, derivparam);
        let f1v = DeriVector2::from_point(store, self.focus1, derivparam);

        cv.subtr(&f1v).normalized().subtr(&f1v.subtr(&p).normalized())
    }

    fn own_params(&self) -> Vec<ParamId> {
        vec![self.vertex.x, self.vertex.y, self.focus1.x, self.focus1.y]
    }

    fn value(&self, store: &ParamStore, u: f64, du: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        let c = DeriVector2::from_point(store, self.vertex, derivparam);
        let f1 = DeriVector2::from_point(store, self.focus1, derivparam);

        let fv = f1.subtr(&c);
        let (f, df) = fv.length_deriv();

        let xdir = fv.normalized();
        let ydir = xdir.rotate90ccw();

        let dirx = xdir.mult_d(u, du).mult_d(u, du).div_d(4.0 * f, 4.0 * df);
        let diry = ydir.mult_d(u, du);
        let dir = dirx.sum(&diry);

        c.sum(&dir)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ArcOfParabola {
    pub parabola: Parabola,
    pub start: Point,
    pub end: Point,
    pub start_angle: ParamId,
    pub end_angle: ParamId,
}

impl Curve for ArcOfParabola {
    fn calculate_normal_from_value(&self, store: &ParamStore, p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2 {
        self.parabola.calculate_normal_from_value(store, p, derivparam)
    }

    fn value(&self, store: &ParamStore, u: f64, du: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        self.parabola.value(store, u, du, derivparam)
    }

    fn own_params(&self) -> Vec<ParamId> {
        let mut params = self.parabola.own_params();
        params.extend([self.start.x, self.start.y, self.end.x, self.end.y, self.start_angle, self.end_angle]);
        params
    }
}

/// de Boor's algorithm, evaluating one basis-spline linear-combination
/// coefficient (or, iterated over a full pole vector `d`, the resulting
/// curve/derivative value) — planegcs's `BSpline::splineValue`, shared by
/// [`BSpline::get_lin_comb_factor`] and every curve/tangent evaluation
/// below. `flatknots` must already be the *flattened* knot vector (each
/// knot value repeated per its multiplicity), `k` the knot-span index such
/// that `flatknots[k] <= x < flatknots[k + 1]`, and `d` the length-`p + 1`
/// window of (possibly weighted) pole coordinates for that span.
pub(crate) fn spline_value(x: f64, k: usize, p: usize, d: &mut [f64], flatknots: &[f64]) -> f64 {
    for r in 1..=p {
        for j in (r..=p).rev() {
            let alpha = (x - flatknots[j + k - p]) / (flatknots[j + 1 + k - r] - flatknots[j + k - p]);
            d[j] = (1.0 - alpha) * d[j - 1] + alpha * d[j];
        }
    }
    if p < d.len() { d[p] } else { 0.0 }
}

/// A rational B-spline (NURBS) curve.
///
/// Two deliberate departures from planegcs's `BSpline`, both because this
/// port targets acadrust's `Spline` entity (`entities/spline.rs` in the
/// `acadrust`/cadcodec crate) as its real consumer, and that entity — like
/// the DXF/DWG formats it round-trips — already stores knots *flattened*
/// (each value repeated per its multiplicity) rather than as planegcs's
/// separate unique-`knots` + `mult` vectors:
///
/// - `knots` here holds the flattened vector directly (planegcs's own
///   `flattenedknots`, computed on demand via `setupFlattenedKnots()` from
///   the other two) — there is no `mult` to carry, and nothing here needs
///   one: [`find_span`](Self::find_span) locates the active knot span
///   directly via the flattened vector (the standard NURBS-book "FindSpan"
///   search), which is exactly equivalent for a well-formed knot vector
///   without ever reconstructing multiplicities.
/// - `knots` are plain `f64`, not [`ParamId`]s. planegcs's *do* register
///   knots as solver parameters (`PushOwnParams` includes them), but no
///   `Value`/`CalculateNormal` formula in the C++ ever actually
///   differentiates with respect to one — they only ever appear
///   dereferenced as plain numbers — so carrying them as [`ParamId`]s here
///   would add bookkeeping with no corresponding behavior to preserve.
///
/// Everything else — poles, weights, `degree`, `periodic`, and critically
/// the *values* `Value`/`CalculateNormal` compute — matches planegcs
/// formula-for-formula, including its real limitations (see
/// [`value`](Curve::value) and [`calculate_normal_at_param`](Curve::calculate_normal_at_param)
/// on [`Curve`]).
#[derive(Debug, Clone)]
pub struct BSpline {
    pub poles: Vec<Point>,
    pub weights: Vec<ParamId>,
    /// Flattened knot vector — see the struct docs. Length must be
    /// `poles.len() + degree + 1`.
    pub knots: Vec<f64>,
    pub start: Point,
    pub end: Point,
    pub degree: usize,
    pub periodic: bool,
}

impl BSpline {
    /// The knot-span index `k` such that `knots[k] <= u < knots[k + 1]`
    /// (clamped to the last valid span at the non-periodic end) — what
    /// planegcs derives indirectly as `startpole + degree` by walking
    /// unique knots and their multiplicities; directly findable here since
    /// `knots` is already flattened.
    pub(crate) fn find_span(&self, u: f64) -> usize {
        let num_poles = self.poles.len();
        if !self.periodic && u >= self.knots[num_poles] {
            return num_poles - 1;
        }
        let mut k = self.degree;
        while k + 1 < num_poles && self.knots[k + 1] <= u {
            k += 1;
        }
        k
    }

    pub(crate) fn pole_x_at(&self, startpole: usize, i: usize) -> ParamId {
        self.poles[(startpole + i) % self.poles.len()].x
    }

    pub(crate) fn pole_y_at(&self, startpole: usize, i: usize) -> ParamId {
        self.poles[(startpole + i) % self.poles.len()].y
    }

    pub(crate) fn weight_at(&self, startpole: usize, i: usize) -> ParamId {
        self.weights[(startpole + i) % self.weights.len()]
    }

    /// The linear-combination coefficient `B_i(x)` such that
    /// `spline(x) = sum(poles[i] * B_i(x))` for a spline of degree `p` —
    /// planegcs's `BSpline::getLinCombFactor`. `k` is the knot-span index
    /// (see [`find_span`](Self::find_span)) and `i` the (window-relative,
    /// i.e. already offset by `startpole`) pole index.
    pub fn get_lin_comb_factor(&self, x: f64, k: usize, i: usize, p: usize) -> f64 {
        let idx_of_pole = i as isize + p as isize - k as isize;
        if idx_of_pole < 0 || idx_of_pole > p as isize {
            return 0.0;
        }
        let mut d = vec![0.0; p + 1];
        d[idx_of_pole as usize] = 1.0;
        spline_value(x, k, p, &mut d, &self.knots);
        d[p]
    }

    /// Homogeneous-coordinate value and its `d/du` at parameter `u`:
    /// `(xw, yw, w, dxw/du, dyw/du, dw/du)` — planegcs's `valueHomogenous`.
    fn value_homogeneous(&self, store: &ParamStore, u: f64) -> (f64, f64, f64, f64, f64, f64) {
        let k = self.find_span(u);
        let startpole = k - self.degree;
        let numpoints = self.degree + 1;

        let mut d = vec![0.0; numpoints];
        for i in 0..numpoints {
            d[i] = store.get(self.pole_x_at(startpole, i)) * store.get(self.weight_at(startpole, i));
        }
        let xw = spline_value(u, k, self.degree, &mut d, &self.knots);
        for i in 0..numpoints {
            d[i] = store.get(self.pole_y_at(startpole, i)) * store.get(self.weight_at(startpole, i));
        }
        let yw = spline_value(u, k, self.degree, &mut d, &self.knots);
        for i in 0..numpoints {
            d[i] = store.get(self.weight_at(startpole, i));
        }
        let w = spline_value(u, k, self.degree, &mut d, &self.knots);

        let denom = |i: usize| self.knots[startpole + i + self.degree] - self.knots[startpole + i];

        let mut sd = vec![0.0; numpoints - 1];
        for i in 1..numpoints {
            sd[i - 1] = (store.get(self.pole_x_at(startpole, i)) * store.get(self.weight_at(startpole, i))
                - store.get(self.pole_x_at(startpole, i - 1)) * store.get(self.weight_at(startpole, i - 1)))
                / denom(i);
        }
        let dxw = self.degree as f64 * spline_value(u, k, self.degree - 1, &mut sd, &self.knots);
        for i in 1..numpoints {
            sd[i - 1] = (store.get(self.pole_y_at(startpole, i)) * store.get(self.weight_at(startpole, i))
                - store.get(self.pole_y_at(startpole, i - 1)) * store.get(self.weight_at(startpole, i - 1)))
                / denom(i);
        }
        let dyw = self.degree as f64 * spline_value(u, k, self.degree - 1, &mut sd, &self.knots);
        for i in 1..numpoints {
            sd[i - 1] = (store.get(self.weight_at(startpole, i)) - store.get(self.weight_at(startpole, i - 1))) / denom(i);
        }
        let dw = self.degree as f64 * spline_value(u, k, self.degree - 1, &mut sd, &self.knots);

        (xw, yw, w, dxw, dyw, dw)
    }
}

impl Curve for BSpline {
    /// Only defined where planegcs's own default falls back to something
    /// other than the zero vector: `p` exactly at the spline's stored
    /// `start` or `end` point, where the tangent is unambiguous (from the
    /// first two, or last two, poles) even without a general de Boor
    /// evaluation. Any other point returns the zero vector, exactly like
    /// upstream's `return {};` — general-point normals on a B-spline need
    /// [`calculate_normal_at_param`](Curve::calculate_normal_at_param) (`u`
    /// known) instead.
    fn calculate_normal_from_value(&self, store: &ParamStore, p: DeriVector2, derivparam: Option<ParamId>) -> DeriVector2 {
        // planegcs checks `mult[0] > degree && mult[last] > degree` (each
        // end knot repeated more than `degree` times — a clamped/open
        // spline). Without a separate `mult` vector (see the struct docs),
        // the equivalent check on the already-flattened, non-decreasing
        // `knots` is: the first `degree + 1` entries (or last `degree + 1`)
        // are all equal, i.e. `knots[0] == knots[degree]`.
        let n = self.knots.len();
        let is_open_ends = self.knots[0] == self.knots[self.degree] && self.knots[n - 1] == self.knots[n - 1 - self.degree];
        if is_open_ends {
            let start = DeriVector2::from_point(store, self.start, None);
            if (p.x - start.x).abs() < f64::EPSILON && (p.y - start.y).abs() < f64::EPSILON {
                let sp = DeriVector2::from_point(store, self.poles[0], derivparam);
                let ep = DeriVector2::from_point(store, self.poles[1], derivparam);
                return ep.subtr(&sp).rotate90ccw();
            }
            let end = DeriVector2::from_point(store, self.end, None);
            if (p.x - end.x).abs() < f64::EPSILON && (p.y - end.y).abs() < f64::EPSILON {
                let sp = DeriVector2::from_point(store, self.poles[self.poles.len() - 2], derivparam);
                let ep = DeriVector2::from_point(store, self.poles[self.poles.len() - 1], derivparam);
                return ep.subtr(&sp).rotate90ccw();
            }
        }
        DeriVector2::new(0.0, 0.0)
    }

    /// Full de Boor tangent/normal-with-derivative at a curve parameter —
    /// planegcs's `BSpline::CalculateNormal(const double*, const double*)`
    /// override (every other curve here uses the [`Curve`] trait's default
    /// for this, but planegcs gives `BSpline` its own full implementation,
    /// so this port does too).
    fn calculate_normal_at_param(&self, store: &ParamStore, param: f64, derivparam: Option<ParamId>) -> DeriVector2 {
        let k = self.find_span(param);
        let startpole = k - self.degree;
        let numpoints = self.degree + 1;

        let (xsum, ysum, wsum, xslopesum, yslopesum, wslopesum) = self.value_homogeneous(store, param);
        let mut result = DeriVector2::new(wsum * xslopesum - wslopesum * xsum, wsum * yslopesum - wslopesum * ysum);

        for i in 0..numpoints {
            let (px, py, w) = (self.pole_x_at(startpole, i), self.pole_y_at(startpole, i), self.weight_at(startpole, i));
            if derivparam != Some(px) && derivparam != Some(py) && derivparam != Some(w) {
                continue;
            }

            let mut d = vec![0.0; numpoints];
            d[i] = 1.0;
            let factor = spline_value(param, startpole + self.degree, self.degree, &mut d, &self.knots);
            let mut sd = vec![0.0; numpoints - 1];
            let denom = |j: usize| self.knots[startpole + j + self.degree] - self.knots[startpole + j];
            if i > 0 {
                sd[i - 1] = 1.0 / denom(i);
            }
            if i < numpoints - 1 {
                sd[i] = -1.0 / denom(i + 1);
            }
            let slopefactor = spline_value(param, startpole + self.degree, self.degree - 1, &mut sd, &self.knots);

            let wi = store.get(w);
            if derivparam == Some(px) {
                result.dx = wi * (wsum * slopefactor - wslopesum * factor);
            } else if derivparam == Some(py) {
                result.dy = wi * (wsum * slopefactor - wslopesum * factor);
            } else if derivparam == Some(w) {
                let (pxv, pyv) = (store.get(px), store.get(py));
                result.dx = self.degree as f64 * (factor * (xslopesum - wslopesum * pxv) - slopefactor * (xsum - wsum * pxv));
                result.dy = self.degree as f64 * (factor * (yslopesum - wslopesum * pyv) - slopefactor * (ysum - wsum * pyv));
            }
            break;
        }

        // The curve parameter itself isn't tracked as a `ParamId` in this
        // port (see the struct docs), so `derivparam == param` — the C++
        // branch computing second-derivative slope sums for that case —
        // has no equivalent here: there is no `ParamId` that could compare
        // equal to it in the first place.
        result.rotate90ccw()
    }

    fn value(&self, store: &ParamStore, u: f64, _du: f64, _derivparam: Option<ParamId>) -> DeriVector2 {
        // Matches planegcs's `BSpline::Value` exactly, including its own
        // surprising signature: `du` and `derivparam` are both ignored
        // (the C++ marks them `/*du*/`, `/*derivparam*/`) — the returned
        // `dx`/`dy` is always the raw tangent d(value)/du, regardless of
        // what derivative the caller actually asked for. A constraint
        // that calls `Curve::value` on a `BSpline` and expects a gradient
        // w.r.t. anything other than `u` itself will get a wrong (if
        // internally consistent with upstream) answer — a real, inherited
        // limitation, not a bug introduced by this port.
        let (xw, yw, w, dxw, dyw, dw) = self.value_homogeneous(store, u);
        DeriVector2::with_deriv(xw / w, yw / w, (w * dxw - dw * xw) / (w * w), (w * dyw - dw * yw) / (w * w))
    }

    fn own_params(&self) -> Vec<ParamId> {
        let mut params = Vec::with_capacity(self.poles.len() * 2 + self.weights.len() + 4);
        for pole in &self.poles {
            params.push(pole.x);
            params.push(pole.y);
        }
        params.extend(self.weights.iter().copied());
        params.extend([self.start.x, self.start.y, self.end.x, self.end.y]);
        params
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-7;

    fn make_point(store: &mut ParamStore, x: f64, y: f64) -> Point {
        Point::new(store.add(x, false), store.add(y, false))
    }

    #[test]
    fn line_value_interpolates_between_endpoints() {
        let mut store = ParamStore::new();
        let p1 = make_point(&mut store, 0.0, 0.0);
        let p2 = make_point(&mut store, 10.0, 0.0);
        let line = Line { p1, p2 };

        let start = line.value(&store, 0.0, 0.0, None);
        assert_eq!((start.x, start.y), (0.0, 0.0));

        let mid = line.value(&store, 0.5, 0.0, None);
        assert_eq!((mid.x, mid.y), (5.0, 0.0));

        let end = line.value(&store, 1.0, 0.0, None);
        assert_eq!((end.x, end.y), (10.0, 0.0));
    }

    #[test]
    fn line_normal_points_left_of_travel_direction() {
        // Walking p1 -> p2 along +x, "left" is +y.
        let mut store = ParamStore::new();
        let p1 = make_point(&mut store, 0.0, 0.0);
        let p2 = make_point(&mut store, 10.0, 0.0);
        let line = Line { p1, p2 };

        let n = line.calculate_normal(&store, p1, None);
        assert!(n.y > 0.0, "expected normal to point toward +y, got {n:?}");
        assert!(n.x.abs() < EPS);
    }

    #[test]
    fn circle_value_traces_the_circle() {
        let mut store = ParamStore::new();
        let center = make_point(&mut store, 1.0, 2.0);
        let rad = store.add(3.0, false);
        let circle = Circle { center, rad };

        let at_zero = circle.value(&store, 0.0, 0.0, None);
        assert!((at_zero.x - 4.0).abs() < EPS && (at_zero.y - 2.0).abs() < EPS);

        let at_half_pi = circle.value(&store, std::f64::consts::FRAC_PI_2, 0.0, None);
        assert!((at_half_pi.x - 1.0).abs() < EPS && (at_half_pi.y - 5.0).abs() < EPS);
    }

    #[test]
    fn circle_value_derivative_wrt_u_matches_the_tangent() {
        // d/du (cx + r cos u, cy + r sin u) = (-r sin u, r cos u); at u=0 that's (0, r).
        let mut store = ParamStore::new();
        let center = make_point(&mut store, 0.0, 0.0);
        let rad = store.add(5.0, false);
        let circle = Circle { center, rad };

        let v = circle.value(&store, 0.0, 1.0, None);
        assert!((v.dx - 0.0).abs() < EPS);
        assert!((v.dy - 5.0).abs() < EPS);
    }

    #[test]
    fn circle_value_derivative_wrt_radius_matches_finite_difference() {
        let mut store = ParamStore::new();
        let center = make_point(&mut store, 0.0, 0.0);
        let rad = store.add(5.0, false);
        let circle = Circle { center, rad };
        let u = 0.7_f64;

        let analytic = circle.value(&store, u, 0.0, Some(rad));

        let h = 1e-6;
        store.set(rad, 5.0 + h);
        let plus = circle.value(&store, u, 0.0, None);
        store.set(rad, 5.0 - h);
        let minus = circle.value(&store, u, 0.0, None);
        let numeric_dx = (plus.x - minus.x) / (2.0 * h);
        let numeric_dy = (plus.y - minus.y) / (2.0 * h);

        assert!((analytic.dx - numeric_dx).abs() < 1e-6);
        assert!((analytic.dy - numeric_dy).abs() < 1e-6);
    }

    #[test]
    fn circle_normal_points_toward_center() {
        let mut store = ParamStore::new();
        let center = make_point(&mut store, 0.0, 0.0);
        let rad = store.add(3.0, false);
        let circle = Circle { center, rad };

        let p = make_point(&mut store, 3.0, 0.0);
        let n = circle.calculate_normal(&store, p, None);
        assert!(n.x < 0.0, "normal at (3,0) should point back toward center, got {n:?}");
        assert!(n.y.abs() < EPS);
    }

    #[test]
    fn arc_delegates_value_and_normal_to_its_circle() {
        let mut store = ParamStore::new();
        let center = make_point(&mut store, 0.0, 0.0);
        let rad = store.add(2.0, false);
        let start = make_point(&mut store, 2.0, 0.0);
        let end = make_point(&mut store, 0.0, 2.0);
        let start_angle = store.add(0.0, false);
        let end_angle = store.add(std::f64::consts::FRAC_PI_2, false);
        let arc = Arc {
            circle: Circle { center, rad },
            start,
            end,
            start_angle,
            end_angle,
        };

        let v = arc.value(&store, 0.0, 0.0, None);
        assert!((v.x - 2.0).abs() < EPS && v.y.abs() < EPS);
    }

    #[test]
    fn derivector2_basic_algebra() {
        let a = DeriVector2::with_deriv(3.0, 4.0, 1.0, 0.0);
        assert!((a.length() - 5.0).abs() < EPS);

        let (len, dlen) = a.length_deriv();
        assert!((len - 5.0).abs() < EPS);
        // d(length)/dparam = (x*dx + y*dy)/len = (3*1 + 4*0)/5 = 0.6
        assert!((dlen - 0.6).abs() < EPS);

        let n = a.normalized();
        assert!((n.length() - 1.0).abs() < EPS);

        let rotated = a.rotate90ccw();
        assert!((rotated.x - (-4.0)).abs() < EPS && (rotated.y - 3.0).abs() < EPS);
        // rotating twice more (180 total) negates the original vector
        let rotated_twice = rotated.rotate90ccw();
        assert!((rotated_twice.x - (-3.0)).abs() < EPS && (rotated_twice.y - (-4.0)).abs() < EPS);
    }

    fn make_ellipse(store: &mut ParamStore, cx: f64, cy: f64, c: f64, radmin: f64) -> Ellipse {
        // Focus at distance c along +x from the centre -- major axis is +x.
        Ellipse {
            center: make_point(store, cx, cy),
            focus1: make_point(store, cx + c, cy),
            radmin: store.add(radmin, false),
        }
    }

    #[test]
    fn ellipse_value_satisfies_the_ellipse_equation_at_several_parameters() {
        let mut store = ParamStore::new();
        let ellipse = make_ellipse(&mut store, 0.0, 0.0, 3.0, 4.0); // c=3, b=4 -> a=5
        let (cx, cy) = (0.0, 0.0);
        let (a, b) = (5.0, 4.0);

        for &u in &[0.0, 0.7, std::f64::consts::FRAC_PI_2, 2.3, 4.5] {
            let v = ellipse.value(&store, u, 0.0, None);
            let dx = v.x - cx;
            let dy = v.y - cy;
            let lhs = (dx / a).powi(2) + (dy / b).powi(2);
            assert!((lhs - 1.0).abs() < EPS, "u={u}: off the ellipse, lhs={lhs}");
        }
    }

    #[test]
    fn ellipse_value_derivative_wrt_radmin_matches_finite_difference() {
        let mut store = ParamStore::new();
        let ellipse = make_ellipse(&mut store, 1.0, -2.0, 3.0, 4.0);
        let u = 1.1_f64;
        let radmin = ellipse.radmin;

        let analytic = ellipse.value(&store, u, 0.0, Some(radmin));

        let h = 1e-6;
        store.set(radmin, 4.0 + h);
        let plus = ellipse.value(&store, u, 0.0, None);
        store.set(radmin, 4.0 - h);
        let minus = ellipse.value(&store, u, 0.0, None);

        assert!((analytic.dx - (plus.x - minus.x) / (2.0 * h)).abs() < 1e-6);
        assert!((analytic.dy - (plus.y - minus.y) / (2.0 * h)).abs() < 1e-6);
    }

    #[test]
    fn arc_of_ellipse_delegates_to_its_ellipse() {
        let mut store = ParamStore::new();
        let ellipse = make_ellipse(&mut store, 0.0, 0.0, 3.0, 4.0);
        let arc = ArcOfEllipse {
            ellipse,
            start: make_point(&mut store, 5.0, 0.0),
            end: make_point(&mut store, 0.0, 4.0),
            start_angle: store.add(0.0, false),
            end_angle: store.add(std::f64::consts::FRAC_PI_2, false),
        };
        let v = arc.value(&store, 0.0, 0.0, None);
        assert!((v.x - 5.0).abs() < EPS && v.y.abs() < EPS);
    }

    #[test]
    fn hyperbola_value_satisfies_the_hyperbola_equation() {
        let mut store = ParamStore::new();
        // c=5, b=4 -> a=3 (3-4-5 triangle keeps this exact in f64).
        let hyperbola = Hyperbola {
            center: make_point(&mut store, 0.0, 0.0),
            focus1: make_point(&mut store, 5.0, 0.0),
            radmin: store.add(4.0, false),
        };
        let (a, b) = (3.0, 4.0);

        for &u in &[0.0, 0.5, -0.8, 1.3] {
            let v = hyperbola.value(&store, u, 0.0, None);
            let lhs = (v.x / a).powi(2) - (v.y / b).powi(2);
            assert!((lhs - 1.0).abs() < 1e-9, "u={u}: off the hyperbola, lhs={lhs}");
        }
    }

    #[test]
    fn parabola_value_satisfies_the_parabola_equation() {
        let mut store = ParamStore::new();
        // Vertex at origin, focus at (0, f) along +y.
        let f = 2.0;
        let parabola = Parabola { vertex: make_point(&mut store, 0.0, 0.0), focus1: make_point(&mut store, 0.0, f) };

        for &u in &[0.0, 1.0, -2.0, 3.5] {
            let v = parabola.value(&store, u, 0.0, None);
            // Value()'s xdir is the unit vector from vertex TOWARD the
            // focus (here +y), with ydir = xdir rotated 90° ccw (here -x),
            // so this local frame gives (x, y) = (-u, u^2/(4f)), not (u, ...).
            assert!((v.x - (-u)).abs() < EPS);
            assert!((v.y - u * u / (4.0 * f)).abs() < EPS);
            // The parabola equation itself (x^2 = 4fy) only depends on x^2,
            // so it holds regardless of that sign.
            assert!((v.x * v.x - 4.0 * f * v.y).abs() < EPS, "u={u}: off the parabola");
        }
    }

    /// A degree-3, all-weights-1, fully-clamped 4-pole B-spline (knots
    /// `[0,0,0,0,1,1,1,1]`) is exactly a cubic Bézier curve over the four
    /// poles — a closed-form oracle independent of `spline_value`'s own
    /// recursion.
    fn make_cubic_bezier_as_bspline(store: &mut ParamStore, poles_xy: [(f64, f64); 4]) -> BSpline {
        let poles: Vec<Point> = poles_xy.iter().map(|&(x, y)| make_point(store, x, y)).collect();
        let weights = vec![store.add(1.0, false); 4];
        BSpline {
            start: poles[0],
            end: poles[3],
            poles,
            weights,
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            degree: 3,
            periodic: false,
        }
    }

    fn cubic_bezier_point(poles: [(f64, f64); 4], u: f64) -> (f64, f64) {
        let b = [
            (1.0 - u).powi(3),
            3.0 * u * (1.0 - u).powi(2),
            3.0 * u * u * (1.0 - u),
            u.powi(3),
        ];
        let x = (0..4).map(|i| b[i] * poles[i].0).sum();
        let y = (0..4).map(|i| b[i] * poles[i].1).sum();
        (x, y)
    }

    #[test]
    fn bspline_value_matches_the_closed_form_cubic_bezier() {
        let mut store = ParamStore::new();
        let poles_xy = [(0.0, 0.0), (1.0, 3.0), (3.0, 3.0), (4.0, 0.0)];
        let bsp = make_cubic_bezier_as_bspline(&mut store, poles_xy);

        for &u in &[0.0, 0.25, 0.5, 0.75, 1.0] {
            let v = bsp.value(&store, u, 0.0, None);
            let (ex, ey) = cubic_bezier_point(poles_xy, u);
            assert!((v.x - ex).abs() < EPS, "u={u}: x mismatch, got {} want {ex}", v.x);
            assert!((v.y - ey).abs() < EPS, "u={u}: y mismatch, got {} want {ey}", v.y);
        }
    }

    #[test]
    fn bspline_value_derivative_wrt_u_matches_finite_difference() {
        let mut store = ParamStore::new();
        let bsp = make_cubic_bezier_as_bspline(&mut store, [(0.0, 0.0), (1.0, 3.0), (3.0, 3.0), (4.0, 0.0)]);
        let u = 0.4_f64;
        let h = 1e-6;

        let analytic = bsp.value(&store, u, 0.0, None);
        let plus = bsp.value(&store, u + h, 0.0, None);
        let minus = bsp.value(&store, u - h, 0.0, None);
        assert!((analytic.dx - (plus.x - minus.x) / (2.0 * h)).abs() < 1e-5);
        assert!((analytic.dy - (plus.y - minus.y) / (2.0 * h)).abs() < 1e-5);
    }

    #[test]
    fn bspline_normal_at_start_and_end_matches_first_and_last_pole_pair() {
        let mut store = ParamStore::new();
        let poles_xy = [(0.0, 0.0), (1.0, 3.0), (3.0, 3.0), (4.0, 0.0)];
        let bsp = make_cubic_bezier_as_bspline(&mut store, poles_xy);

        // Query at the exact stored `start`/`end` points (not a freshly
        // computed `value()`, which could differ by float roundoff and so
        // fail `calculate_normal_from_value`'s exact-equality check).
        let n_start = bsp.calculate_normal(&store, bsp.start, None);
        // Tangent at start is pole1 - pole0 = (1,3); normal is that rotated 90ccw = (-3,1).
        assert!((n_start.x - (-3.0)).abs() < EPS && (n_start.y - 1.0).abs() < EPS, "{n_start:?}");

        let n_end = bsp.calculate_normal(&store, bsp.end, None);
        // Tangent at end is pole3 - pole2 = (1,-3); normal is that rotated 90ccw = (3,1).
        assert!((n_end.x - 3.0).abs() < EPS && (n_end.y - 1.0).abs() < EPS, "{n_end:?}");
    }

    #[test]
    fn bspline_normal_at_param_matches_the_tangent_direction() {
        let mut store = ParamStore::new();
        let bsp = make_cubic_bezier_as_bspline(&mut store, [(0.0, 0.0), (1.0, 3.0), (3.0, 3.0), (4.0, 0.0)]);
        let u = 0.5;

        let v = bsp.value(&store, u, 1.0, None); // dx,dy = tangent direction
        let n = bsp.calculate_normal_at_param(&store, u, None);
        // Normal is the tangent rotated 90ccw: (-dy, dx).
        let tangent_len = (v.dx * v.dx + v.dy * v.dy).sqrt();
        assert!((n.x / n.x.hypot(n.y) - (-v.dy / tangent_len)).abs() < 1e-6);
        assert!((n.y / n.x.hypot(n.y) - (v.dx / tangent_len)).abs() < 1e-6);
    }
}
