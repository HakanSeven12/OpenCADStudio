//! Parametric-chain engine (api 14, the v2 generic contract): ramps as
//! declared data, evaluated generically, audited at the seams, staged as
//! reference-only display carriers.
//!
//! The host knows NO curve species. A chain carries, per segment, a curve
//! definition (`length`, the driving-quantity ramp, the cross-slope data)
//! plus the continuity fields (start point and direction); the driving
//! quantity is the plan curvature in the plan plane and the profile slope
//! in the profile plane — the record model's two sheets. Which quantity a
//! ramp drives and which record-model laws it restates are the declaring
//! plugin's own semantics; this module executes the generic contract only:
//!
//! - the ramp is evaluated and integrated over the three `Ramp` arms
//!   (`ramp_eval` / `ramp_integral`; the polynomial and piecewise arms in
//!   closed form, the sampled arm by its grid's own trapezoid law with a
//!   precomputed stage-time cumulative table so drag-time re-integration
//!   touches only the moved tail);
//! - the plan plane integrates heading theta(s) = theta0 + L*W(s/L) into
//!   position by composite 16-node Gauss-Legendre, subdivided by the total
//!   tangent deflection (one sub-interval per half-radian); the profile
//!   plane is exact: x = station0 + s, h(s) = h0 + L*W(s/L);
//! - exact carriers derive generically, WITHOUT a species branch: a plan
//!   ramp that is a degree-0 polynomial (constant curvature) stages a
//!   Line (curvature zero) or an exact Arc; a profile ramp that is an
//!   at-most-quadratic polynomial stages an exact clamped Hermite spline
//!   of degree up to 3 built from the segment's two end states by ONE
//!   builder (the ramp's height integral is then a polynomial of degree
//!   up to 3 in the station, exactly a clamped Bernstein form);
//! - everything else stages measured-deviation LwPolyline chords within
//!   the declared tolerance (seeded by the driving quantity's realized
//!   rate, refined by measurement — never a guessed shape); the plan
//!   integrator carries a swing budget (65 536 half-radian sub-intervals)
//!   and a law past it refuses — a hang cannot hide behind legal data;
//! - chains are audited seam-to-seam at the declared tolerance — the
//!   instance's `PluginClassInstance.tolerance` (or the raw staging call's
//!   tolerance argument): the point seam reads it as meters, the plan's
//!   bearing seam as radians, the profile's slope seam as the gradient
//!   family (`tolerance * (1 + g^2)`) — and refusals name the audited pair; a chain
//!   that would stage more display entities than `MAX_PLUGIN_LIVE_ENTITIES`
//!   publishes nothing; non-finite or structurally broken ramp data
//!   refuses at validation (the host refuses nothing it cannot state
//!   generically — species semantics are the plugin's own checks).
//!
//! A HELIX entity is never produced: the codec's Helix is a 3D
//! constant-pitch space spiral, while a plan ramp curve is a 2D law —
//! staging one for the other would be mathematically wrong.

use codec::entities::{Arc, EntityType, Line, LwPolyline, LwVertex, Spline};
use codec::types::{Vector2, Vector3};
use ocs_plugin_api::host::{
    MAX_PLUGIN_LIVE_ENTITIES, ParametricChain, ParametricPlane, ParametricSample,
    ParametricSegment, Ramp,
};
use std::f64::consts::PI;

const TWO_PI: f64 = std::f64::consts::TAU;

/// Upper bound on a declared instance tolerance: the seam audit and the
/// chord carriers both read the plugin's own declaration, but a
/// declaration looser than this ceiling can only ever mask a genuinely
/// broken chain (and coarsen chords past usefulness). Refuse at the
/// boundary; the ceiling is a host law, not a negotiable datum.
pub const MAX_DECLARED_TOLERANCE: f64 = 1.0e-2;

/// Upper chord count per segment: once the refinement loop would exceed
/// this, the tolerance is judged unattainable and staging refuses instead
/// of looping.
const MAX_CHORDS_PER_SEGMENT: usize = 50_000;

/// The plan integrator's swing budget (the caps law applied to the
/// quadrature): the sub-interval count is one per half-radian of tangent
/// swing, at most this many. A declared plan law whose total tangent swing
/// over one segment exceeds the budget refuses at validation — a 16-node
/// quadrature across more sub-intervals than this is a hanging path, not a
/// curve the host agrees to evaluate (the structural gates alone allow the
/// data; the budget keeps the surface hang-free). 65536 half-radian
/// windows = ~10 000 full turns per segment, far past any real alignment.
const MAX_GL_SUB_INTERVALS: usize = 65_536;

/// Normalize a start direction into [0, 2*pi). Declared directions are
/// +-2*pi-legal (any winding is accepted by the record layer); the engine
/// normalizes once at the segment boundary.
fn normalize_angle(angle: f64) -> f64 {
    angle.rem_euclid(TWO_PI)
}

/// Unsigned angular distance between two (possibly differently wound)
/// angles.
fn angle_distance(a: f64, b: f64) -> f64 {
    let d = (normalize_angle(a) - normalize_angle(b)).rem_euclid(TWO_PI);
    if d > PI {
        TWO_PI - d
    } else {
        d
    }
}

// ==== the generic ramp machinery ==========================================

/// Evaluate one `Ramp` arm at `xi` (clamped to [0, 1]). Polynomial:
/// ascending Horner over `coeffs[0..=degree]`. Piecewise: linear
/// interpolation through the active knots, clamped before the first and
/// after the last. Sampled: uniform-grid linear lookup.
pub(crate) fn ramp_eval(ramp: &Ramp, xi: f64) -> f64 {
    let xi = xi.clamp(0.0, 1.0);
    match ramp {
        Ramp::Polynomial { degree, coeffs } => {
            let d = (*degree).min(3) as usize;
            let mut acc = 0.0;
            for i in (0..=d).rev() {
                acc = acc * xi + coeffs[i];
            }
            acc
        }
        Ramp::Piecewise { count, knots } => {
            let active = (*count).clamp(1, knots.len() as u32) as usize;
            // The piecewise-LINEAR law: interpolated between the active
            // knots, clamped before the first and after the last.
            let knots = &knots[..active];
            if knots.len() == 1 {
                return knots[0].1;
            }
            if xi <= knots[0].0 {
                return knots[0].1;
            }
            for pair in knots.windows(2) {
                let (xa, va) = pair[0];
                let (xb, vb) = pair[1];
                if xb <= xa {
                    continue;
                }
                if xi <= xb {
                    let frac = (xi - xa) / (xb - xa);
                    return va + (vb - va) * frac;
                }
            }
            knots[knots.len() - 1].1
        }
        Ramp::Sampled { n, values } => {
            let count = (*n).clamp(1, values.len() as u32) as usize;
            if count == 1 {
                return values[0];
            }
            let t = xi * (count - 1) as f64;
            let i0 = (t.floor() as usize).min(count - 2).max(0);
            let frac = t - i0 as f64;
            values[i0] + (values[i0 + 1] - values[i0]) * frac
        }
    }
}

/// Integrate one `Ramp` arm over [0, xi] (xi clamped to [0, 1]).
/// Polynomial: the antiderivative in closed form. Piecewise: the per-
/// interval quadratic accumulation of the linear law. Sampled: the grid's
/// own trapezoid law (the audit's deviation floor derives from the table
/// spacing, engine-verified).
pub(crate) fn ramp_integral(ramp: &Ramp, xi: f64) -> f64 {
    let xi = xi.clamp(0.0, 1.0);
    match ramp {
        Ramp::Polynomial { degree, coeffs } => {
            let d = (*degree).min(3) as usize;
            let mut acc = 0.0;
            for i in (0..=d).rev() {
                acc = (acc + coeffs[i] / (i as f64 + 1.0)) * xi;
            }
            // the loop yields Σ coeffs[i]·xi^(i+1)/(i+1) in ascending order
            acc
        }
        Ramp::Piecewise { count, knots } => {
            let active = (*count).clamp(1, knots.len() as u32) as usize;
            piecewise_integral(&knots[..active], xi)
        }
        Ramp::Sampled { n, values } => {
            let count = (*n).clamp(1, values.len() as u32) as usize;
            if count == 1 {
                // A one-point table is the constant law: eval answers the
                // single value everywhere, so its integral is the linear
                // accumulation of that value (consistent with `ramp_eval`).
                return values[0] * xi;
            }
            let step = 1.0 / (count - 1) as f64;
            let full = (xi / step).floor() as usize;
            let full = full.min(count - 1);
            let mut total = 0.0;
            for i in 0..full {
                total += (values[i] + values[i + 1]) * 0.5;
            }
            total *= step;
            let done_at = full as f64 * step;
            let tail = xi - done_at;
            if tail > 0.0 && full < count - 1 {
                total += (values[full] + values[full + 1]) * 0.5 * tail;
            }
            total
        }
    }
}

/// The antiderivative of one piecewise-linear law (the ramp values through
/// the active knots, clamped before the first and after the last knot).
/// The tails' constant spans accumulate too: a table's knots may sit
/// anywhere inside [0, 1], and the clamped value is the law's own reading
/// on the same spans the eval clamps. A single knot is the constant law.
fn piecewise_integral(knots: &[(f64, f64)], xi: f64) -> f64 {
    if knots.is_empty() {
        return 0.0;
    }
    let (x0, v0) = knots[0];
    if knots.len() == 1 {
        return v0 * xi;
    }
    if xi <= x0 {
        return v0 * xi;
    }
    let mut acc = v0 * x0;
    for pair in knots.windows(2) {
        let (xa, va) = pair[0];
        let (xb, vb) = pair[1];
        let span = xb - xa;
        if span <= 0.0 {
            continue;
        }
        if xi <= xb {
            let frac = (xi - xa) / span;
            acc += va * (xi - xa) + (vb - va) * (xi - xa) * frac * 0.5;
            return acc;
        }
        acc += va * span + (vb - va) * span * 0.5;
    }
    let (xlast, vlast) = knots[knots.len() - 1];
    acc += vlast * (xi - xlast).max(0.0);
    acc
}

/// Validate one ramp arm's STRUCTURE (the generic law only): every active
/// value finite; a polynomial degree at most 3; piecewise knots strictly
/// ascending in xi within [0, 1] with 1..=6 active; a sampled table of
/// 1..=4096 finite values. Law semantics stay the declaring plugin's own.
fn validate_ramp(ramp: &Ramp) -> Result<(), String> {
    match ramp {
        Ramp::Polynomial { degree, coeffs } => {
            if *degree > 3 {
                return Err(format!(
                    "a polynomial ramp degree of {degree} exceeds the at-most-cubic envelope (3)"
                ));
            }
            let d = *degree as usize;
            if !coeffs[..d].iter().all(|c| c.is_finite()) {
                return Err("a polynomial ramp coefficient is not finite".to_owned());
            }
        }
        Ramp::Piecewise { count, knots } => {
            if *count == 0 || *count > 6 {
                return Err(format!(
                    "a piecewise ramp declares {count} active knots; the window is 1..=6"
                ));
            }
            let active = *count as usize;
            let mut prev_xi = -1.0;
            for (xi, value) in &knots[..active] {
                if !xi.is_finite() || !value.is_finite() {
                    return Err("a piecewise ramp knot is not finite".to_owned());
                }
                if !(0.0..=1.0).contains(xi) {
                    return Err(format!("a piecewise ramp knot sits at xi {xi}, outside [0, 1]"));
                }
                if *xi <= prev_xi {
                    return Err(format!(
                        "a piecewise ramp's knots do not ascend (xi {xi} after {prev_xi})"
                    ));
                }
                prev_xi = *xi;
            }
        }
        Ramp::Sampled { n, values } => {
            if *n == 0 || *n > 4096 {
                return Err(format!(
                    "a sampled ramp declares {n} slots; the window is 1..=4096"
                ));
            }
            if (*n as usize) > values.len() {
                return Err(format!(
                    "a sampled ramp declares {} slots but carries {} values",
                    n,
                    values.len()
                ));
            }
            if !values[..*n as usize].iter().all(|v| v.is_finite()) {
                return Err("a sampled ramp value is not finite".to_owned());
            }
        }
    }
    Ok(())
}

/// One prepared driving-quantity ramp: the declared data plus the
/// stage-time cumulative integral table the Sampled arm precomputes
/// (Vec<f64> per Sampled slot edge) so drag-time re-integration touches
/// only the moved tail. Carried in the staging intermediates — never
/// persisted anywhere.
#[derive(Debug, Clone)]
struct PreparedRamp {
    ramp: Ramp,
    /// Sampled arm only: cumulative[i] = the ramp's integral from 0 up to
    /// the i-th slot edge, i = 0..slot_count (cumulative[0] = 0).
    cumulative: Option<Vec<f64>>,
}

impl PreparedRamp {
    /// Prepare one ramp; the callers validated the data already.
    fn new(ramp: &Ramp) -> PreparedRamp {
        let cumulative = match ramp {
            Ramp::Sampled { n, values } => {
                let count = (*n).clamp(1, values.len() as u32) as usize;
                if count >= 2 {
                    let step = 1.0 / (count - 1) as f64;
                    let mut table = Vec::with_capacity(count);
                    table.push(0.0);
                    for i in 0..count - 1 {
                        let last = table[i];
                        table.push(last + (values[i] + values[i + 1]) * 0.5 * step);
                    }
                    Some(table)
                } else {
                    None
                }
            }
            _ => None,
        };
        PreparedRamp {
            ramp: ramp.clone(),
            cumulative,
        }
    }

    /// Evaluate at xi (clamped); the eval law is shared with `ramp_eval`.
    fn eval(&self, xi: f64) -> f64 {
        ramp_eval(&self.ramp, xi)
    }

    /// Integrate over [0, xi] (clamped). The Sampled arm reads the
    /// cumulative table: one lookup plus one exacting trapezoid tail; the
    /// other arms stay closed-form.
    fn integral(&self, xi: f64) -> f64 {
        let xi = xi.clamp(0.0, 1.0);
        let Ramp::Sampled { n, values } = &self.ramp else {
            return ramp_integral(&self.ramp, xi);
        };
        let count = (*n).clamp(1, values.len() as u32) as usize;
        if count == 1 {
            return values[0] * xi;
        }
        let Some(cumulative) = &self.cumulative else {
            return ramp_integral(&self.ramp, xi);
        };
        let step = 1.0 / (count - 1) as f64;
        let full = ((xi / step).floor() as usize).min(count - 1);
        let mut total = cumulative[full];
        let done_at = full as f64 * step;
        let tail = xi - done_at;
        if tail > 0.0 && full < count - 1 {
            total += (values[full] + values[full + 1]) * 0.5 * tail;
        }
        total
    }

    /// The ramp's end value (the law's own reading at xi = 1).
    fn end_value(&self) -> f64 {
        self.eval(1.0)
    }
}

// ==== validation, evaluation, audit =======================================

/// Validated derived data of one declared segment.
#[derive(Debug, Clone)]
struct Vitals {
    /// Declared length: plan = the arc length; profile = the horizontal
    /// run (the record models' own readings).
    length: f64,
    /// Plan: the normalized bearing. Profile: the declared start direction
    /// (the continuity read; the slope ramp is the law's own truth).
    theta_start: f64,
    /// The driving-quantity ramp as prepared data (with the Sampled arm's
    /// stage-time cumulative integral table).
    ramp: PreparedRamp,
}

/// Validate one declared segment generically and derive the evaluation
/// vitals. Refusals name the offending field; no species checks exist.
fn validate_segment(plane: ParametricPlane, segment: &ParametricSegment) -> Result<Vitals, String> {
    if !segment.start_point.iter().all(|c| c.is_finite()) {
        return Err("the declared start point is not finite".to_owned());
    }
    if !segment.start_direction.is_finite() {
        return Err("the declared start direction is not finite".to_owned());
    }
    let def = &segment.def;
    if !def.length.is_finite() {
        return Err("the declared length is not finite".to_owned());
    }
    if def.length <= 0.0 {
        return Err(format!(
            "the declared length ({}) must be greater than zero; a zero-length terminator is a plugin staging detail, not a host contract",
            def.length
        ));
    }
    validate_ramp(&def.ramp).map_err(|e| format!("the driving ramp is not structural: {e}"))?;
    if let Some(cross) = &def.cross_slope {
        validate_ramp(cross).map_err(|e| format!("the cross-slope ramp is not structural: {e}"))?;
    }
    let ramp = PreparedRamp::new(&def.ramp);
    let theta_start = match plane {
        // The plan plane's bearing normalizes once at the segment boundary.
        ParametricPlane::Plan => {
            // The integrator's swing budget: the segment's total tangent
            // deflection bounds the sub-interval count one-to-one; a law
            // past the budget is a hang, not a curve — refuse here where
            // every evaluation path funnels through (the audit, the
            // sampling, the staging).
            let swing = (def.length * ramp.integral(1.0)).abs();
            if swing > MAX_GL_SUB_INTERVALS as f64 * PI * 0.5 {
                return Err(format!(
                    "the plan law's tangent swing ({swing:.0} rad over this segment) exceeds the \
                     integrator's swing budget ({MAX_GL_SUB_INTERVALS} sub-intervals); refuse the \
                     segment, refuse the chain"
                ));
            }
            normalize_angle(segment.start_direction)
        }
        // The profile plane's direction is the continuity read; the angle
        // carries over as declared.
        ParametricPlane::Profile => segment.start_direction,
    };
    Ok(Vitals {
        length: def.length,
        theta_start,
        ramp,
    })
}

/// 16-node Gauss-Legendre nodes (on [-1, 1]) and weights, hard-coded
/// constants so evaluation is bit-deterministic. The nodes are quoted
/// verbatim from the reference table (digit-for-digit), hence the allow.
#[allow(clippy::excessive_precision)]
const GL16_Z: [f64; 16] = [
    -0.98940093499164995,
    -0.94457502307323258,
    -0.86563120238783174,
    -0.75540440835500303,
    -0.61787624440264375,
    -0.45801677765722739,
    -0.28160355077925891,
    -0.09501250983763744,
    0.09501250983763744,
    0.28160355077925891,
    0.45801677765722739,
    0.61787624440264375,
    0.75540440835500303,
    0.86563120238783174,
    0.94457502307323258,
    0.98940093499164995,
];
const GL16_W: [f64; 16] = [
    0.0271524594117541,
    0.0622535239386479,
    0.0951585116824928,
    0.1246289712555339,
    0.1495959888165767,
    0.1691565193950025,
    0.1826034150449236,
    0.1894506104550685,
    0.1894506104550685,
    0.1826034150449236,
    0.1691565193950025,
    0.1495959888165767,
    0.1246289712555339,
    0.0951585116824928,
    0.0622535239386479,
    0.0271524594117541,
];

/// Composite 16-node Gauss-Legendre over [0, domain], subdivided by the
/// integrand's total phase swing (one sub-interval per half-radian of
/// deflection). Deterministic and allocation-light.
fn integrate(mut f: impl FnMut(f64) -> f64, domain: f64, total_swing: f64) -> f64 {
    if domain == 0.0 {
        return 0.0;
    }
    let mut sub_intervals = 1.0f64.max(total_swing.abs() / (PI * 0.5)).ceil() as usize;
    sub_intervals = sub_intervals.max(1);
    let h = domain / sub_intervals as f64;
    let mut sum = 0.0;
    for i in 0..sub_intervals {
        let a = i as f64 * h;
        for (z, w) in GL16_Z.iter().zip(GL16_W.iter()) {
            let t = a + 0.5 * h * (z + 1.0);
            sum += w * f(t);
        }
    }
    sum * 0.5 * h
}

/// Evaluate one (already validated) segment at local position u. ONE
/// evaluator, no species branches: the plan plane integrates theta(s) =
/// theta0 + L*W(s/L) into (x, y) by the composite integrator; the profile
/// plane is exact (x = station0 + s, h = h0 + L*W(s/L), the slope the
/// ramp's own value).
fn evaluate_segment_at(
    plane: ParametricPlane,
    segment: &ParametricSegment,
    vitals: &Vitals,
    u: f64,
) -> ParametricSample {
    let l = vitals.length;
    match plane {
        ParametricPlane::Plan => {
            // The integrand runs in the segment's LOCAL frame (the
            // deflection only, no bearing yet); the sub-interval count
            // follows the deflection's own swing; the world point rotates
            // into the declared bearing afterwards.
            let deflection_of = |t: f64| l * vitals.ramp.integral(t / l);
            let swing = deflection_of(u);
            let x = integrate(|t| deflection_of(t).cos(), u, swing);
            let y = integrate(|t| deflection_of(t).sin(), u, swing);
            let dir_angle = normalize_angle(vitals.theta_start + deflection_of(u));
            let cos0 = vitals.theta_start.cos();
            let sin0 = vitals.theta_start.sin();
            ParametricSample {
                point: [
                    segment.start_point[0] + cos0 * x - sin0 * y,
                    segment.start_point[1] + sin0 * x + cos0 * y,
                ],
                direction: [dir_angle.cos(), dir_angle.sin()],
            }
        }
        ParametricPlane::Profile => {
            let xi = u / l;
            let g = vitals.ramp.eval(xi);
            let height = segment.start_point[1] + l * vitals.ramp.integral(xi);
            let norm = (1.0 + g * g).sqrt();
            ParametricSample {
                point: [segment.start_point[0] + u, height],
                direction: [1.0 / norm, g / norm],
            }
        }
    }
}

/// Audit every declared segment pair seam-to-seam: the evaluated end of
/// segment i must equal the declared start of segment i+1 in point, and
/// in direction. The continuity bound is the caller's OWN tolerance — the
/// declaring instance's `PluginClassInstance.tolerance` (or the raw
/// `stage_parametric_chain` tolerance argument): one number, the record
/// model's declared accuracy, read as meters for the point seam, radians
/// for the plan's bearing seam, and the gradient's relative family for
/// the profile's slope seam. The host adds no hard-coded seam constant.
fn audit_chain(chain: &ParametricChain, tolerance: f64) -> Result<Vec<Vitals>, String> {
    let mut vitals = Vec::with_capacity(chain.segments.len());
    for (index, segment) in chain.segments.iter().enumerate() {
        let v =
            validate_segment(chain.plane, segment).map_err(|e| format!("segment {index}: {e}"))?;
        vitals.push(v);
    }
    for (index, (pair, v)) in chain.segments.windows(2).zip(vitals.iter()).enumerate() {
        let segment = &pair[0];
        let next = &pair[1];
        let end = evaluate_segment_at(chain.plane, segment, v, segment.def.length);
        if !next.start_point.iter().all(|c| c.is_finite()) {
            return Err(format!(
                "seam segment {index} to {}: the declared start point is not finite",
                index + 1
            ));
        }
        let gap = ((end.point[0] - next.start_point[0]).powi(2)
            + (end.point[1] - next.start_point[1]).powi(2))
        .sqrt();
        if gap > tolerance {
            return Err(format!(
                "seam segment {index} to {}: the evaluated end ({:.6}, {:.6}) is {gap:.9} m away from the next segment's declared start ({:.6}, {:.6}); the chain is not continuous",
                index + 1,
                end.point[0],
                end.point[1],
                next.start_point[0],
                next.start_point[1]
            ));
        }
        match chain.plane {
            ParametricPlane::Plan => {
                let realized = end.direction[1].atan2(end.direction[0]);
                let turn = angle_distance(realized, next.start_direction);
                if turn > tolerance {
                    return Err(format!(
                        "seam segment {index} to {}: the evaluated end direction ({realized:.12} rad) does not restate the declared start direction ({:.12} rad) of the next segment; the chain is not tangentially continuous",
                        index + 1,
                        next.start_direction
                    ));
                }
            }
            ParametricPlane::Profile => {
                // The realized end slope vs the next segment's declared
                // start slope — the slope ramp's own start value (the law
                // the records pair with the segment's end gradient).
                let realized_gradient = end.direction[1] / end.direction[0];
                let declared = ramp_eval(&next.def.ramp, 0.0);
                if (realized_gradient - declared).abs()
                    > tolerance * (1.0 + declared.abs() * declared.abs())
                {
                    return Err(format!(
                        "seam segment {index} to {}: the evaluated end gradient ({realized_gradient:.9}) does not restate the declared start gradient ({declared:.9}) of the next segment",
                        index + 1
                    ));
                }
            }
        }
    }
    Ok(vitals)
}

/// Sample a declared chain at chain position s (measured along the chain's
/// declared segment lengths). Pure evaluation: no document state, no undo,
/// no publish. Outside the chain (s < 0 or past the end) the answer is
/// `None`.
pub fn sample_chain(chain: &ParametricChain, s: f64) -> Result<Option<ParametricSample>, String> {
    if !s.is_finite() {
        return Err("sample position is not finite".to_owned());
    }
    let mut vitals = Vec::with_capacity(chain.segments.len());
    let mut total = 0.0;
    for (index, segment) in chain.segments.iter().enumerate() {
        let v =
            validate_segment(chain.plane, segment).map_err(|e| format!("segment {index}: {e}"))?;
        total += v.length;
        vitals.push(v);
    }
    if s < 0.0 || total == 0.0 || s > total {
        return Ok(None);
    }
    let mut acc = 0.0;
    for (index, segment) in chain.segments.iter().enumerate() {
        let span = vitals[index].length;
        if s <= acc + span || index + 1 == chain.segments.len() {
            let u = (s - acc).clamp(0.0, span);
            return Ok(Some(evaluate_segment_at(
                chain.plane,
                segment,
                &vitals[index],
                u,
            )));
        }
        acc += span;
    }
    Ok(None)
}

// ==== carriers ============================================================

/// Which carrier answered for one staged segment (diagnostics).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StagedCarrier {
    /// One exact standard entity.
    Exact,
    /// A chord polyline with this many chords.
    Chords(usize),
}

/// The exact carrier of one segment, when the ramp class admits one
/// GENERICALLY: plan ramps that are degree-0 polynomials (constant
/// curvature) stage lines (curvature zero) or exact arcs; profile ramps
/// that are at-most-quadratic polynomials stage exact clamped Hermite
/// splines from the segment's two end states (the slope's height integral
/// stays at-most-cubic in the station). Everything else stages measured
/// chords within the declared tolerance.
fn exact_carrier(
    plane: ParametricPlane,
    segment: &ParametricSegment,
    vitals: &Vitals,
) -> Option<EntityType> {
    let Ramp::Polynomial { degree, coeffs } = &segment.def.ramp else {
        return None;
    };
    match plane {
        ParametricPlane::Plan => {
            if *degree != 0 {
                // A non-constant plan curvature integrates to the spiral
                // family: position is transcendental, chords carry it.
                return None;
            }
            if coeffs[0] == 0.0 {
                Some(EntityType::Line(plan_line(segment, vitals)))
            } else {
                Some(EntityType::Arc(plan_arc(segment, vitals, 1.0 / coeffs[0])))
            }
        }
        ParametricPlane::Profile => match *degree {
            0 => Some(EntityType::Line(profile_line(segment, vitals))),
            // The spline degree follows the ramp: slope-linear = degree 2,
            // slope-quadratic = degree 3, both exact for the class.
            1 | 2 => Some(EntityType::Spline(profile_hermite_spline(segment, vitals))),
            // A cubic slope law's height integral is quartic: beyond the
            // at-most-cubic Hermite envelope, the chords carry it.
            _ => None,
        },
    }
}

/// The plan LINE carrier: endpoints the exact evaluations (the declared
/// start, the segment's evaluated end — theta unchanged, curvature zero).
fn plan_line(segment: &ParametricSegment, vitals: &Vitals) -> Line {
    let dir = [vitals.theta_start.cos(), vitals.theta_start.sin()];
    Line::from_points(
        Vector3::new(segment.start_point[0], segment.start_point[1], 0.0),
        Vector3::new(
            segment.start_point[0] + dir[0] * vitals.length,
            segment.start_point[1] + dir[1] * vitals.length,
            0.0,
        ),
    )
}

/// The profile LINE carrier (the constant-slope law): endpoints the exact
/// evaluations.
fn profile_line(segment: &ParametricSegment, vitals: &Vitals) -> Line {
    let l = segment.def.length;
    Line::from_points(
        Vector3::new(segment.start_point[0], segment.start_point[1], 0.0),
        Vector3::new(
            segment.start_point[0] + l,
            segment.start_point[1] + l * vitals.ramp.end_value(),
            0.0,
        ),
    )
}

/// The plan ARC carrier for a constant curvature ramp: the exact arc
/// entity of the sheet circle. The record's signed curvature convention
/// (positive = CCW turn) maps to the entity's positive radius with the
/// start/end angles chosen so the drawn point set is identical: positive
/// (CCW) sweep from theta0 - pi/2 forward by L/|R|; negative (CW) draws
/// the same point set with the span angles recomputed from the CW side.
fn plan_arc(segment: &ParametricSegment, vitals: &Vitals, r: f64) -> Arc {
    let theta0 = vitals.theta_start;
    // The center sits |R| perpendicular to the tangent, the turn side the
    // signed radius/curvature encodes: center = P0 + R*(-sin, cos).
    let center = [
        segment.start_point[0] - r * theta0.sin(),
        segment.start_point[1] + r * theta0.cos(),
    ];
    let (start_angle, end_angle) = if r > 0.0 {
        let a0 = theta0 - 0.5 * PI;
        let sweep = segment.def.length / r;
        (a0, a0 + sweep)
    } else {
        let a1 = theta0 + 0.5 * PI;
        let sweep = segment.def.length / v_abs(r);
        (a1 - sweep, a1)
    };
    Arc::from_center_radius_angles(
        Vector3::new(center[0], center[1], 0.0),
        v_abs(r),
        start_angle,
        end_angle,
    )
}

fn v_abs(v: f64) -> f64 {
    v.abs()
}

/// The generic Hermite-from-endpoints builder for the profile polynomial
/// class (the exact carrier; no species branch): the interior control
/// points derive from the segment's two END STATES (the evaluated point
/// pair and the slope ramp's end values). Degree 2 (a linear slope ramp):
/// the mid control sits on the START tangent's line at the mid station
/// (NOT the curve's own midpoint). Degree 3 (a quadratic slope ramp): both
/// end tangents lift their third points. The slope's height integral is
/// then a polynomial of degree up to 3 in the station, which the clamped
/// form reproduces EXACTLY — the tests pin the from-end-states control
/// points against the law's own Bernstein identity to machine eps.
fn profile_hermite_spline(segment: &ParametricSegment, vitals: &Vitals) -> Spline {
    let l = segment.def.length;
    let station0 = segment.start_point[0];
    let h0 = segment.start_point[1];
    let g0 = vitals.ramp.eval(0.0);
    let g1 = vitals.ramp.eval(1.0);
    let end_height = h0 + l * vitals.ramp.integral(1.0);
    let Ramp::Polynomial { degree, .. } = &segment.def.ramp else {
        unreachable!("the hermite builder is keyed by the polynomial class")
    };
    // The derivative state in the (station, elevation) sheet is (1, g);
    // the Bernstein end-offsets are (L/d)·(1, g) in plane coordinates.
    let points: Vec<Vector3> = match *degree {
        1 => vec![
            Vector3::new(station0, h0, 0.0),
            Vector3::new(station0 + l * 0.5, h0 + g0 * l * 0.5, 0.0),
            Vector3::new(station0 + l, end_height, 0.0),
        ],
        2 => vec![
            Vector3::new(station0, h0, 0.0),
            Vector3::new(station0 + l / 3.0, h0 + g0 * l / 3.0, 0.0),
            Vector3::new(station0 + 2.0 * l / 3.0, end_height - g1 * l / 3.0, 0.0),
            Vector3::new(station0 + l, end_height, 0.0),
        ],
        _ => unreachable!("the profile Hermite class is slope degree 1 or 2"),
    };
    // The spline's degree follows the ramp's: a slope-linear law's height
    // integral is quadratic (degree 2), a quadratic slope's is cubic
    // (degree 3) — the exact clamped form for each.
    let d = (*degree).min(3) + 1;
    let mut spline = Spline::from_control_points(d as i32, points);
    spline.flags.planar = true;
    spline.begin_tangent = Vector3::new(1.0, g0, 0.0);
    spline.end_tangent = Vector3::new(1.0, g1, 0.0);
    spline
}

/// The ramp arm's own label (the chord-plan error paths name it).
fn ramp_arm_label(ramp: &Ramp) -> &'static str {
    match ramp {
        Ramp::Polynomial { .. } => "polynomial",
        Ramp::Piecewise { .. } => "piecewise",
        Ramp::Sampled { .. } => "sampled",
    }
}

/// Build the display entities for a declared, audited chain. Every staged
/// entity carries the chain's layer (when set); the reference-only
/// designation is applied by the host session, not here. Returns the
/// entities and the per-segment carriers.
pub fn build_display_entities(
    chain: &ParametricChain,
    tolerance: f64,
) -> Result<(Vec<EntityType>, Vec<StagedCarrier>), String> {
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err("a positive finite tolerance is required".to_owned());
    }
    if tolerance > MAX_DECLARED_TOLERANCE {
        return Err(format!(
            "the declared tolerance {tolerance} exceeds the host ceiling \
             {MAX_DECLARED_TOLERANCE}: a declaration that loose can only mask a broken chain"
        ));
    }
    let vitals = audit_chain(chain, tolerance)?;
    // Cap pre-check (the published ceiling): nothing is staged when the
    // chain's display footprint would exceed the live-entity cap.
    let mut estimate = 0usize;
    for (segment, v) in chain.segments.iter().zip(vitals.iter()) {
        estimate += match exact_carrier(chain.plane, segment, v) {
            Some(_) => 1,
            None => chord_seed(chain.plane, &v.ramp, v.length, tolerance),
        };
    }
    if estimate > MAX_PLUGIN_LIVE_ENTITIES {
        return Err(format!(
            "a {}-segment chain at tolerance {tolerance} needs {estimate} display entities, above the MAX_PLUGIN_LIVE_ENTITIES ({MAX_PLUGIN_LIVE_ENTITIES}) ceiling; nothing staged",
            chain.segments.len()
        ));
    }
    let mut entities: Vec<EntityType> = Vec::new();
    let mut carriers = Vec::with_capacity(chain.segments.len());
    for (index, segment) in chain.segments.iter().enumerate() {
        let v = &vitals[index];
        match exact_carrier(chain.plane, segment, v) {
            Some(entity) => {
                entities.push(entity);
                carriers.push(StagedCarrier::Exact);
            }
            None => {
                let eval = |u: f64| evaluate_segment_at(chain.plane, segment, v, u);
                let seed = chord_seed(chain.plane, &v.ramp, v.length, tolerance);
                let (polyline, count) = chord_polyline(
                    &eval,
                    v.length,
                    seed,
                    tolerance,
                    &format!(
                        "segment {index} ({} ramp)",
                        ramp_arm_label(&segment.def.ramp)
                    ),
                )
                .map_err(|e| format!("segment {index}: {e}"))?;
                entities.push(EntityType::LwPolyline(polyline));
                carriers.push(StagedCarrier::Chords(count));
            }
        }
    }
    if !chain.layer.is_empty() {
        for entity in &mut entities {
            set_common_layer(entity, &chain.layer);
        }
    }
    Ok((entities, carriers))
}

/// Apply the chain layer to one entity.
fn set_common_layer(entity: &mut EntityType, layer: &str) {
    if !layer.is_empty() {
        entity.common_mut().layer = layer.to_string();
    }
}

/// Chord count seed from the driving quantity's realized rate (the sagitta
/// law, generalized): a chord of span ds on a curve whose realized
/// curvature bound is |k| bulges at most ds^2*|k|/8, so
/// n >= L*sqrt(|k|/(8*tolerance)). Plan: the driving quantity IS the
/// curvature. Profile: the graph's curvature bound is the slope's realized
/// rate over the run. The measured deviation check is the real law; this
/// bound only seeds it.
fn chord_seed(plane: ParametricPlane, ramp: &PreparedRamp, length: f64, tolerance: f64) -> usize {
    let slices = 48usize;
    let mut max_value = 0.0f64;
    let mut max_rate = 0.0f64;
    let mut prev = ramp.eval(0.0);
    for k in 1..=slices {
        let xi = k as f64 / slices as f64;
        let value = ramp.eval(xi);
        max_value = max_value.max(value.abs());
        max_rate = max_rate.max((value - prev).abs() * slices as f64 / length);
        prev = value;
    }
    let bound = match plane {
        ParametricPlane::Plan => max_value,
        ParametricPlane::Profile => max_rate,
    };
    if bound <= 0.0 {
        return 2;
    }
    let n = (length * (bound / (8.0 * tolerance)).sqrt()).ceil() as usize;
    n.max(2)
}

/// Chord polyline with the max-deviation tolerance contract: seeded from
/// the sagitta bound, refined by measured deviation until the measured
/// maximum sits within the declared tolerance. Refuses with the reached
/// numbers when the refinement ceiling keeps the fit above the tolerance.
fn chord_polyline(
    eval: &dyn Fn(f64) -> ParametricSample,
    length: f64,
    mut count: usize,
    tolerance: f64,
    label: &str,
) -> Result<(LwPolyline, usize), String> {
    loop {
        let polyline = chords_for(eval, length, count);
        let worst = measured_deviation(&polyline, eval, length);
        if worst <= tolerance {
            return Ok((polyline, count));
        }
        if count >= MAX_CHORDS_PER_SEGMENT {
            return Err(format!(
                "{label}: {count} chords still deviate by {worst:.3e} m at a {tolerance} tolerance; the tolerance is unattainable, nothing staged for it"
            ));
        }
        count = count.saturating_mul(2).max(4);
    }
}

fn chords_for(eval: &dyn Fn(f64) -> ParametricSample, length: f64, count: usize) -> LwPolyline {
    let mut polyline = LwPolyline::new();
    for i in 0..=count {
        let u = length * i as f64 / count as f64;
        let sample = eval(u);
        polyline.add_vertex(LwVertex::new(Vector2::new(sample.point[0], sample.point[1])));
    }
    polyline
}

/// The measured max deviation of the chord polyline vs the true curve:
/// three quarter-points sampled per chord, the distance to the chord line.
fn measured_deviation(
    polyline: &LwPolyline,
    eval: &dyn Fn(f64) -> ParametricSample,
    length: f64,
) -> f64 {
    let count = polyline.vertices.len().saturating_sub(1).max(1);
    let vertices: Vec<(f64, f64)> = polyline
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect();
    let mut worst = 0.0f64;
    for i in 0..count {
        let (ax, ay) = vertices[i];
        let (bx, by) = vertices[i + 1];
        let (dx, dy) = (bx - ax, by - ay);
        let chord_len = (dx * dx + dy * dy).sqrt();
        let (ux, uy) = if chord_len == 0.0 {
            (1.0, 0.0)
        } else {
            (dx / chord_len, dy / chord_len)
        };
        for k in 1..4 {
            let frac = k as f64 / 4.0;
            let u = length * (i as f64 + frac) / count as f64;
            let sample = eval(u);
            let px = sample.point[0] - ax;
            let py = sample.point[1] - ay;
            let along = px * ux + py * uy;
            let dist = ((px - along * ux).powi(2) + (py - along * uy).powi(2)).sqrt();
            worst = worst.max(dist);
        }
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::host::CurveSegmentDef;

    // ==== fixtures ========================================================

    fn segment_of(
        plane: ParametricPlane,
        def: CurveSegmentDef,
        start_point: [f64; 2],
        start_direction: f64,
    ) -> ParametricSegment {
        let _ = plane;
        ParametricSegment {
            def,
            start_point,
            start_direction,
        }
    }

    fn ramp_const(value: f64) -> Ramp {
        Ramp::Polynomial {
            degree: 0,
            coeffs: [value, 0.0, 0.0, 0.0],
        }
    }

    fn ramp_linear(start: f64, end: f64) -> Ramp {
        Ramp::Polynomial {
            degree: 1,
            coeffs: [start, end - start, 0.0, 0.0],
        }
    }

    fn def_of(length: f64, ramp: Ramp) -> CurveSegmentDef {
        CurveSegmentDef {
            length,
            ramp,
            cross_slope: None,
        }
    }

    fn plan_line_def(length: f64) -> CurveSegmentDef {
        def_of(length, ramp_const(0.0))
    }

    fn plan_arc_def(length: f64, radius: f64) -> CurveSegmentDef {
        def_of(length, ramp_const(1.0 / radius))
    }

    /// The linear-curvature law (kappa from 0 to `end_kappa`): the
    /// spiral's own ramp.
    fn plan_spiral_def(length: f64, end_kappa: f64) -> CurveSegmentDef {
        def_of(length, ramp_linear(0.0, end_kappa))
    }

    fn profile_const_slope_def(length: f64, gradient: f64) -> CurveSegmentDef {
        def_of(length, ramp_const(gradient))
    }

    fn profile_linear_slope_def(length: f64, g0: f64, g1: f64) -> CurveSegmentDef {
        def_of(length, ramp_linear(g0, g1))
    }

    fn profile_quadratic_slope_def(length: f64, c0: f64, c1: f64, c2: f64) -> CurveSegmentDef {
        def_of(length, Ramp::Polynomial { degree: 2, coeffs: [c0, c1, c2, 0.0] })
    }

    fn piecewise_ramp(knots: &[(f64, f64)]) -> Ramp {
        let mut table = [(0.0, 0.0); 6];
        for (i, (xi, value)) in knots.iter().enumerate() {
            table[i] = (*xi, *value);
        }
        Ramp::Piecewise {
            count: knots.len() as u32,
            knots: table,
        }
    }

    fn sampled_ramp(values: &[f64]) -> Ramp {
        Ramp::Sampled {
            n: values.len() as u32,
            values: values.to_vec(),
        }
    }

    /// The tests' independent oracle for the linear-curvature law: the
    /// Fresnel position series of theta(s) = b*s^2 (b = end_kappa/(2*L),
    /// the signed curvature included). Anchors the fixtures' welded seams
    /// and cross-checks the engine's Gauss-Legendre integrator.
    fn spiral_pos(b: f64, s: f64) -> (f64, f64) {
        fn factorial(n: usize) -> f64 {
            (1..=n as u64).fold(1u128, |acc: u128, v| acc.saturating_mul(v as u128)) as f64
        }
        let mut x = 0.0f64;
        let mut y = 0.0f64;
        for k in 0..12usize {
            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
            x += sign * b.powi(2 * k as i32) * s.powi(4 * k as i32 + 1)
                / (factorial(2 * k) * (4.0 * k as f64 + 1.0));
            y += sign * b.powi(2 * k as i32 + 1) * s.powi(4 * k as i32 + 3)
                / (factorial(2 * k + 1) * (4.0 * k as f64 + 3.0));
        }
        (x, y)
    }

    /// The plan fixture: [curvature-zero 60 -> curvature 0 -> 1/(-300)
    /// over 60 -> curvature-zero 40], the closing line welded at the
    /// spiral's own series end, at its end bearing.
    fn demo_plan_chain() -> ParametricChain {
        let k1 = 1.0 / -300.0;
        let (x, y) = spiral_pos(k1 / (2.0 * 60.0), 60.0);
        ParametricChain {
            plane: ParametricPlane::Plan,
            segments: vec![
                segment_of(ParametricPlane::Plan, plan_line_def(60.0), [0.0, 0.0], 0.0),
                segment_of(ParametricPlane::Plan, plan_spiral_def(60.0, k1), [60.0, 0.0], 0.0),
                segment_of(
                    ParametricPlane::Plan,
                    plan_line_def(40.0),
                    [60.0 + x, y],
                    k1 * 60.0 / 2.0,
                ),
            ],
            layer: "PLUGIN_DISPLAY".to_string(),
        }
    }

    /// The profile fixture: [constant slope 0.01 over 100 -> slope-linear
    /// 0.01 to 0.06 over 200 -> constant slope 0.06 over 80], entering at
    /// (0, 0), the joints (100, 1.0) and (300, 8.0).
    fn demo_profile_chain() -> ParametricChain {
        ParametricChain {
            plane: ParametricPlane::Profile,
            segments: vec![
                segment_of(
                    ParametricPlane::Profile,
                    profile_const_slope_def(100.0, 0.01),
                    [0.0, 0.0],
                    0.01f64.atan(),
                ),
                segment_of(
                    ParametricPlane::Profile,
                    profile_linear_slope_def(200.0, 0.01, 0.06),
                    [100.0, 1.0],
                    0.01f64.atan(),
                ),
                segment_of(
                    ParametricPlane::Profile,
                    profile_const_slope_def(80.0, 0.06),
                    [300.0, 8.0],
                    0.06f64.atan(),
                ),
            ],
            layer: "PLUGIN_PROFILE".to_string(),
        }
    }

    /// A one-segment chain around `segment`, in its own plane.
    fn single_chain(plane: ParametricPlane, segment: &ParametricSegment) -> ParametricChain {
        ParametricChain {
            plane,
            segments: vec![segment.clone()],
            layer: String::new(),
        }
    }

    // ==== the ramp machinery (the stage-A parity set, rewired to the
    // ==== switched-over names) ===========================================

    #[test]
    fn polynomial_ramp_evals_asc_coeffs() {
        // v(xi) = 2 + 3·xi: v(0.5) = 3.5; ∫[0,0.5] = 2·0.5 + 3·0.5²/2 = 1.375.
        let ramp = ramp_linear(2.0, 5.0);
        assert!((ramp_eval(&ramp, 0.5) - 3.5).abs() < 1e-12);
        assert!((ramp_integral(&ramp, 0.5) - 1.375).abs() < 1e-12);
        // A constant kappa=1 integrates to the full unit over [0,1].
        let ramp = ramp_const(1.0);
        assert!((ramp_integral(&ramp, 1.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn piecewise_ramp_evals_and_integrates_quadratically() {
        // A triangle: knots (0,0), (0.5,1), (1,0) — value peak 0.5 at xi
        // 0.25; the exact integral (linear law) = the triangle area = 0.5.
        let ramp = piecewise_ramp(&[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)]);
        assert!((ramp_eval(&ramp, 0.25) - 0.5).abs() < 1e-12);
        assert!((ramp_integral(&ramp, 1.0) - 0.5).abs() < 1e-12);
        assert!((ramp_integral(&ramp, 0.5) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn sampled_ramp_matches_the_piecewise_within_the_grid() {
        // n=201 samples over the same triangle law; the integral within the
        // trapezoid law's local error, the value within one grid step.
        let ramp = Ramp::Sampled {
            n: 201,
            values: (0..201)
                .map(|i| {
                    let xi = i as f64 / 200.0;
                    if xi <= 0.5 {
                        2.0 * xi
                    } else {
                        2.0 - 2.0 * xi
                    }
                })
                .collect(),
        };
        let eval = ramp_eval(&ramp, 0.25);
        assert!((eval - 0.5).abs() < 1.0 / 200.0);
        let total = ramp_integral(&ramp, 1.0);
        assert!((total - 0.5).abs() < 1e-4, "sampled trapezoid integral: {total}");
    }

    #[test]
    fn ramp_eval_and_integral_are_bit_deterministic_and_clamped() {
        let ramp = Ramp::Polynomial {
            degree: 2,
            coeffs: [1.0, -3.0, 2.0, 0.0],
        };
        let a = ramp_eval(&ramp, 0.3);
        let b = ramp_eval(&ramp, 0.3);
        assert_eq!(a.to_bits(), b.to_bits(), "the eval must be bit-stable");
        let ia = ramp_integral(&ramp, 0.3);
        let ib = ramp_integral(&ramp, 0.3);
        assert_eq!(ia.to_bits(), ib.to_bits(), "the integral must be bit-stable");
        assert!((ramp_eval(&ramp, -2.0) - ramp_eval(&ramp, 0.0)).abs() < 1e-15);
        assert!((ramp_eval(&ramp, 5.0) - ramp_eval(&ramp, 1.0)).abs() < 1e-15);
        assert!((ramp_integral(&ramp, 5.0) - ramp_integral(&ramp, 1.0)).abs() < 1e-15);
    }

    #[test]
    fn prepared_cumulative_matches_the_from_scratch_trapezoid() {
        // The stage-time table's whole point: the same law, only the tail
        // per move. The prepared and the from-scratch integrals must agree
        // to the accumulation's floating-point width.
        let values: Vec<f64> = (0..257)
            .map(|i| (i as f64 * 0.7).sin().abs() + 0.05)
            .collect();
        let ramp = Ramp::Sampled {
            n: 257,
            values: values.clone(),
        };
        let prepared = PreparedRamp::new(&ramp);
        for i in 0..=32u32 {
            let xi = i as f64 / 32.0;
            let a = prepared.integral(xi);
            let b = ramp_integral(&ramp, xi);
            assert!(
                (a - b).abs() <= 1e-12 * b.abs().max(0.01),
                "prepared {a} vs from-scratch {b} at xi {xi}"
            );
        }
        let end = prepared.integral(1.0);
        assert!(
            (end - ramp_integral(&ramp, 1.0)).abs() < 1e-12,
            "the span end: {end}"
        );
    }

    #[test]
    fn degenerate_one_point_ramps_stay_constant_laws() {
        // A one-point sampled table and a one-knot piecewise law are the
        // CONSTANT law: the value everywhere, the integral its accumulation
        // (consistent eval/integral pairs, no 0.0 placeholders).
        let sampled = Ramp::Sampled { n: 1, values: vec![0.02] };
        assert!((ramp_eval(&sampled, 0.3) - 0.02).abs() < 1e-15);
        assert!((ramp_eval(&sampled, 1.0) - 0.02).abs() < 1e-15);
        assert!((ramp_integral(&sampled, 1.0) - 0.02).abs() < 1e-15);
        assert!((ramp_integral(&sampled, 0.25) - 0.005).abs() < 1e-15);
        let piecewise = piecewise_ramp(&[(0.5, 0.02)]);
        assert!((ramp_eval(&piecewise, 0.3) - 0.02).abs() < 1e-15);
        assert!((ramp_integral(&piecewise, 1.0) - 0.02).abs() < 1e-15);
        assert!((ramp_integral(&piecewise, 0.25) - 0.005).abs() < 1e-15);
    }

    // ==== exact carriers ==================================================

    #[test]
    fn plan_constant_ramps_stages_line_and_arc_exact() {
        // Curvature zero: the exact line; constant curvature: the exact
        // arc. The signed-curvature convention maps to the exact circle:
        // the center rides the turn's own side (CW here: below the start),
        // the drawn angular span = L/|R|, and the drawn stroke's two
        // endpoints cover the curve's own (a CW turn presents the same
        // point set with the stroke order reversed — an arc entity is a
        // CCW-set writer).
        let line_chain = ParametricChain {
            plane: ParametricPlane::Plan,
            segments: vec![segment_of(ParametricPlane::Plan, plan_line_def(60.0), [40.0, 7.0], 0.3)],
            layer: String::new(),
        };
        let (entities, carriers) = build_display_entities(&line_chain, 1e-3).unwrap();
        assert!(matches!(carriers[0], StagedCarrier::Exact));
        let EntityType::Line(line) = &entities[0] else {
            panic!("the zero-curvature plan law stages a line");
        };
        let (ex, ey) = (line.end.x, line.end.y);
        assert!((ex - (40.0 + 60.0 * 0.3f64.cos())).abs() < 1e-12);
        assert!((ey - (7.0 + 60.0 * 0.3f64.sin())).abs() < 1e-12);

        let arc_chain = ParametricChain {
            plane: ParametricPlane::Plan,
            segments: vec![segment_of(
                ParametricPlane::Plan,
                plan_arc_def(60.0, -300.0),
                [0.0, 0.0],
                0.0,
            )],
            layer: String::new(),
        };
        let (entities, carriers) = build_display_entities(&arc_chain, 1e-3).unwrap();
        assert!(matches!(carriers[0], StagedCarrier::Exact));
        let EntityType::Arc(arc) = &entities[0] else {
            panic!("the constant-curvature plan law stages an arc");
        };
        assert!((arc.radius - 300.0).abs() < 1e-9);
        assert!((arc.center.x - 0.0).abs() < 1e-9);
        assert!((arc.center.y - (-300.0)).abs() < 1e-9);
        assert!(
            (arc.end_angle - arc.start_angle - 60.0 / 300.0).abs() < 1e-12,
            "the drawn span = the sweep"
        );
        let stroke0 = [
            arc.center.x + arc.radius * arc.start_angle.cos(),
            arc.center.y + arc.radius * arc.start_angle.sin(),
        ];
        let stroke1 = [
            arc.center.x + arc.radius * arc.end_angle.cos(),
            arc.center.y + arc.radius * arc.end_angle.sin(),
        ];
        let vitals = audit_chain(&arc_chain, 1.0e-3).unwrap();
        let evaluated = evaluate_segment_at(
            ParametricPlane::Plan,
            &arc_chain.segments[0],
            &vitals[0],
            60.0,
        );
        let covers = |p: [f64; 2], q: [f64; 2]| {
            (p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9
        };
        let curve_start = [0.0f64, 0.0];
        let curve_end = [evaluated.point[0], evaluated.point[1]];
        assert!(
            (covers(stroke0, curve_start) && covers(stroke1, curve_end))
                || (covers(stroke0, curve_end) && covers(stroke1, curve_start)),
            "the drawn set covers the curve's own endpoints with the exact span"
        );
    }

    #[test]
    fn profile_linear_slope_stages_exact_degree_2_hermite() {
        // The crest's own law as data: slope 0.01 -> 0.06 over 200, the
        // start at (100, 1). The height integral is quadratic in the
        // station; the clamped degree-2 Bernstein form is EXACT: the mid
        // control sits on the START tangent's line at the mid station, the
        // end control = the law's own integrated height.
        let seg = segment_of(
            ParametricPlane::Profile,
            profile_linear_slope_def(200.0, 0.01, 0.06),
            [100.0, 1.0],
            0.01f64.atan(),
        );
        let chain = single_chain(ParametricPlane::Profile, &seg);
        let (entities, carriers) = build_display_entities(&chain, 1e-3).unwrap();
        assert!(matches!(carriers[0], StagedCarrier::Exact));
        let EntityType::Spline(spline) = &entities[0] else {
            panic!("the linear slope law stages a spline");
        };
        assert_eq!(spline.degree, 2);
        assert_eq!(spline.control_points.len(), 3);
        let (x0, h0, l) = (100.0, 1.0, 200.0);
        let (g0, g1) = (0.01, 0.06);
        let mid = h0 + g0 * l * 0.5;
        let end_height = h0 + l * (g0 + g1) * 0.5;
        assert!((spline.control_points[0].x - x0).abs() < 1e-12);
        assert!((spline.control_points[0].y - h0).abs() < 1e-12);
        assert!((spline.control_points[1].x - (x0 + l * 0.5)).abs() < 1e-12);
        assert!(
            (spline.control_points[1].y - mid).abs() < 1e-12,
            "the mid control = the start-tangent line's mid value"
        );
        assert!((spline.control_points[2].x - (x0 + l)).abs() < 1e-12);
        assert!(
            (spline.control_points[2].y - end_height).abs() < 1e-12,
            "the end control = the law's own height integral"
        );
    }

    #[test]
    fn profile_quadratic_slope_stages_exact_degree_3_hermite() {
        // The ramp g(xi) = c0 + c1*xi + c2*xi^2: the height integral
        // h(s) = h0 + c0*s + c1*s^2/(2L) + c2*s^3/(3L^2) is CUBIC in the
        // station. The ONE from-end-states builder must reproduce the law's
        // own Bernstein identity to machine eps (the machine-eps
        // exactness the contract declares).
        let (c0, c1, c2) = (0.0, 0.1, -0.1);
        let l = 100.0f64;
        let seg = segment_of(
            ParametricPlane::Profile,
            profile_quadratic_slope_def(l, c0, c1, c2),
            [0.0, 0.0],
            0.0,
        );
        let chain = single_chain(ParametricPlane::Profile, &seg);
        let (entities, carriers) = build_display_entities(&chain, 1e-3).unwrap();
        assert!(matches!(carriers[0], StagedCarrier::Exact));
        let EntityType::Spline(spline) = &entities[0] else {
            panic!("the quadratic slope law stages a spline");
        };
        assert_eq!(spline.degree, 3);
        assert_eq!(spline.control_points.len(), 4);
        // The law's own power form in t = s/L: h = A0 + A1*t + A2*t^2 + A3*t^3.
        let a0 = 0.0f64;
        let a1 = c0 * l;
        let a2 = c1 * l / 2.0;
        let a3 = c2 * l / 3.0;
        // The power->Bernstein identities for degree 3.
        let b = [
            a0,
            a0 + a1 / 3.0,
            a0 + 2.0 * a1 / 3.0 + a2 / 3.0,
            a0 + a1 + a2 + a3,
        ];
        for (i, b_y) in b.iter().enumerate() {
            let x = l * i as f64 / 3.0;
            assert!((spline.control_points[i].x - x).abs() < 1e-12, "control x {i}");
            assert!(
                (spline.control_points[i].y - b_y).abs() < 1e-12,
                "control y {i}: {} vs {b_y}",
                spline.control_points[i].y
            );
        }
    }

    #[test]
    fn profile_constant_slope_stages_a_line() {
        let seg = segment_of(
            ParametricPlane::Profile,
            profile_const_slope_def(50.0, 0.02),
            [10.0, 4.0],
            0.02f64.atan(),
        );
        let chain = single_chain(ParametricPlane::Profile, &seg);
        let (entities, carriers) = build_display_entities(&chain, 1e-3).unwrap();
        assert!(matches!(carriers[0], StagedCarrier::Exact));
        let EntityType::Line(line) = &entities[0] else {
            panic!("the constant slope stages a line");
        };
        let (ex, ey) = (line.end.x, line.end.y);
        assert!((ex - 60.0).abs() < 1e-12);
        assert!((ey - 5.0).abs() < 1e-12);
    }

    // ==== audits ==========================================================

    #[test]
    fn audit_seams_weld_across_ramp_arms() {
        // Plan chain [polynomial-degree-0 line -> piecewise law -> sampled
        // law]: the seams propagate from the engine's own sampling (the
        // piecewise law integrates 0.0115 over its unit span), the audit
        // gates, and the carriers answer exact + chords.
        let front = ParametricChain {
            plane: ParametricPlane::Plan,
            segments: vec![
                segment_of(
                    ParametricPlane::Plan,
                    plan_line_def(60.0),
                    [0.0, 0.0],
                    0.0,
                ),
                segment_of(
                    ParametricPlane::Plan,
                    def_of(
                        60.0,
                        piecewise_ramp(&[(0.0, 0.001), (0.5, 0.02), (1.0, 0.005)]),
                    ),
                    [60.0, 0.0],
                    0.0,
                ),
            ],
            layer: String::new(),
        };
        let joint = sample_chain(&front, 120.0)
            .unwrap()
            .expect("the front's own end samples");
        let realized = joint.direction[1].atan2(joint.direction[0]);
        assert!((realized - 60.0 * 0.0115).abs() < 1e-9, "the piecewise law's end bearing {realized}");
        let values: Vec<f64> = (0..41)
            .map(|i| 0.005 - 0.001 * (i as f64 / 40.0))
            .collect();
        let mut chain = front.clone();
        chain.segments.push(segment_of(
            ParametricPlane::Plan,
            def_of(40.0, sampled_ramp(&values)),
            joint.point,
            realized,
        ));
        audit_chain(&chain, 1.0e-3).expect("the welded three-law chain audits green");
        let (entities, carriers) = build_display_entities(&chain, 1e-3).expect("builds");
        assert_eq!(entities.len(), 3);
        assert!(matches!(carriers[0], StagedCarrier::Exact));
        assert!(matches!(carriers[1], StagedCarrier::Chords(_)));
        assert!(matches!(carriers[2], StagedCarrier::Chords(_)));
    }

    #[test]
    fn seam_refusal_names_the_pair() {
        let mut chain = demo_plan_chain();
        // Break the seam: the closing line starts 1e-4 m away from the
        // spiral's evaluated end.
        chain.segments[2].start_point[0] += 1e-2;
        let err = build_display_entities(&chain, 1e-3).unwrap_err();
        assert!(err.contains("seam segment 1 to 2"), "names the audited pair: {err}");
        assert!(err.contains("not continuous"), "{err}");

        // A tangential break refuses too: flip the closing line's direction.
        let mut chain = demo_plan_chain();
        chain.segments[2].start_direction = (1.0 / -300.0) * 60.0 / 2.0 + 1e-2;
        let err = build_display_entities(&chain, 1e-3).unwrap_err();
        assert!(
            err.contains("tangentially continuous"),
            "the direction break is named: {err}"
        );
    }

    #[test]
    fn profile_seam_reads_the_next_ramp_start() {
        // The profile seam pairs the realized end slope with the NEXT
        // segment's own ramp start value: a declared start slope that the
        // ramp contradicts refuses with the audited pair named.
        let chain = ParametricChain {
            plane: ParametricPlane::Profile,
            segments: vec![
                segment_of(
                    ParametricPlane::Profile,
                    profile_const_slope_def(100.0, 0.01),
                    [0.0, 0.0],
                    0.01f64.atan(),
                ),
                segment_of(
                    ParametricPlane::Profile,
                    profile_linear_slope_def(200.0, 0.02, 0.06),
                    [100.0, 1.0],
                    0.02f64.atan(),
                ),
            ],
            layer: String::new(),
        };
        let err = build_display_entities(&chain, 1e-3).unwrap_err();
        assert!(
            err.contains("seam segment 0 to 1"),
            "the audited pair names: {err}"
        );
        assert!(
            err.contains("does not restate the declared start gradient"),
            "{err}"
        );
    }

    #[test]
    fn sample_chain_outside_answers_none() {
        let chain = demo_profile_chain();
        assert!(sample_chain(&chain, -0.1).unwrap().is_none());
        let total: f64 = chain.segments.iter().map(|s| s.def.length).sum();
        assert!(sample_chain(&chain, total + 1e-3).unwrap().is_none());
        assert!(sample_chain(&chain, total).unwrap().is_some(), "the end samples");
        assert!(sample_chain(&chain, f64::NAN).is_err());
        assert!(sample_chain(&chain, f64::INFINITY).is_err());
    }

    #[test]
    fn profile_samples_restate_the_mean_gradient_law() {
        // The crest's mid law is the record model's own mean-gradient
        // reading: h = h0 + L*(g0+g1)/2 at the run end; the sample the
        // engine answers restates it to machine width.
        let chain = demo_profile_chain();
        let sample = sample_chain(&chain, 300.0).unwrap().unwrap();
        assert!((sample.point[0] - 300.0).abs() < 1e-9);
        assert!((sample.point[1] - 8.0).abs() < 1e-9, "the crest height");
        let gradient = sample.direction[1] / sample.direction[0];
        assert!((gradient - 0.06).abs() < 1e-12, "the end slope");
    }

    #[test]
    fn v2_ramp_validation_refuses_structural_faults() {
        // Length window.
        let mut seg = segment_of(ParametricPlane::Plan, plan_line_def(60.0), [0.0, 0.0], 0.0);
        seg.def.length = 0.0;
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("must be greater than zero"), "{err}");
        seg.def.length = -5.0;
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("must be greater than zero"), "{err}");

        // Degree cap.
        let mut seg =
            segment_of(ParametricPlane::Plan, plan_line_def(60.0), [0.0, 0.0], 0.0);
        seg.def.ramp = Ramp::Polynomial { degree: 4, coeffs: [0.1; 4] };
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("at-most-cubic envelope"), "{err}");

        // Piecewise windows.
        let mut seg =
            segment_of(ParametricPlane::Plan, plan_line_def(60.0), [0.0, 0.0], 0.0);
        seg.def.ramp = Ramp::Piecewise {
            count: 7,
            knots: [(0.0, 0.0); 6],
        };
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("the window is 1..=6"), "{err}");
        seg.def.ramp = piecewise_ramp(&[(0.0, 0.1), (0.5, 0.2), (0.4, 0.3)]);
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("do not ascend"), "{err}");
        seg.def.ramp = piecewise_ramp(&[(0.0, 0.1), (1.5, 0.3)]);
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("outside [0, 1]"), "{err}");

        // Sampled windows.
        let mut seg =
            segment_of(ParametricPlane::Plan, plan_line_def(60.0), [0.0, 0.0], 0.0);
        seg.def.ramp = Ramp::Sampled { n: 0, values: vec![] };
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("the window is 1..=4096"), "{err}");
        seg.def.ramp = Ramp::Sampled { n: 5000, values: vec![0.0; 5000] };
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("the window is 1..=4096"), "{err}");
        seg.def.ramp = Ramp::Sampled { n: 4, values: vec![0.1, 0.2, 0.3] };
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("but carries 3 values"), "{err}");
        seg.def.ramp = Ramp::Sampled {
            n: 3,
            values: vec![0.1, f64::NAN, 0.3],
        };
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("not finite"), "{err}");

        // Cross-slope data validates by the same rules.
        seg.def.ramp = ramp_const(0.1);
        seg.def.cross_slope = Some(Ramp::Sampled { n: 0, values: vec![] });
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("cross-slope ramp is not structural"), "{err}");

        // Continuity fields.
        seg.def.cross_slope = None;
        seg.start_point = [f64::NAN, 0.0];
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("start point is not finite"), "{err}");
        seg.start_point = [0.0, 0.0];
        seg.start_direction = f64::NAN;
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(err.contains("start direction is not finite"), "{err}");
    }

    #[test]
    fn the_declared_tolerance_has_a_hard_ceiling() {
        // The ceiling is a host law: exactly 1e-2 stages, anything
        // looser refuses naming the ceiling — the honors split at the
        // boundary. The seam audit itself stays the caller's own
        // tolerance; the ceiling guards the DECLARATION, not the audit.
        let chain = demo_plan_chain();
        build_display_entities(&chain, MAX_DECLARED_TOLERANCE)
            .expect("the ceiling itself is a legal declaration");
        let err = build_display_entities(&chain, 1.0e-1)
            .err()
            .expect("a declaration past the ceiling refuses");
        assert!(err.contains("exceeds the host ceiling"), "{err}");
    }

    #[test]
    fn seam_audit_honours_the_declared_tolerance() {
        // The seam bound = the caller's own tolerance, not a host
        // constant: a 5e-4 m gap between two welded segments stages at
        // a 1e-3 tolerance and refuses at a 1e-6 one; the audited pair
        // keeps its name either way. The raw surface's own argument
        // drives it identically.
        let mut chain = demo_profile_chain();
        chain.segments[1].start_point[1] += 5.0e-4;
        let err = build_display_entities(&chain, 1.0e-6)
            .err()
            .expect("the tight tolerance refuses");
        assert!(err.contains("seam segment 0 to 1"), "{err}");
        assert!(err.contains("not continuous"), "{err}");
        let (entities, _) = build_display_entities(&chain, 1.0e-3)
            .expect("the declared tolerance honours the gap");
        assert_eq!(entities.len(), 3);
        audit_chain(&chain, 1.0e-9)
            .expect_err("a 1e-9 demand refuses the same 5e-4 gap");
    }

    #[test]
    fn plan_law_swings_beyond_the_integrator_budget_refuses() {
        // kappa = 1e4 over 40 m swings the tangent ~4e5 rad inside ONE
        // segment: structurally legal ramp data, but past the quadrature's
        // sub-interval budget the host refuses instead of spinning the
        // session away (the chord-seed estimate still sits under its own
        // cap for this fixture, so the refusal comes from the swing law —
        // the budget the chord cap's shape twins).
        let seg = segment_of(
            ParametricPlane::Plan,
            def_of(40.0, ramp_const(1.0e4)),
            [0.0, 0.0],
            0.0,
        );
        let err = audit_chain(&single_chain(ParametricPlane::Plan, &seg), 1.0e-3).unwrap_err();
        assert!(
            err.contains("exceeds the integrator's swing budget"),
            "{err}"
        );
    }

    // ==== grip-edit re-derivation =========================================

    #[test]
    fn grip_edits_rederive_within_the_audited_contract() {
        // The grip-edit correctness arc (engine level): a grip drag revises
        // the declared ramp data, the honest re-declare propagates the
        // moved JOINT from the engine's own sampling (no duplicated math),
        // and the audited chain law holds on the edited set before any
        // carrier is rebuilt. No timing asserts — the bench owns the
        // budgets; this pins the edit -> re-derive -> audit -> carrier
        // path.

        // ---- the profile crest: ramp data in, joints propagated ----
        let vertical = demo_profile_chain();
        audit_chain(&vertical, 1.0e-3).expect("the crest fixture audits (the mean-gradient seams are exact)");

        // THE DRAG: the crest's end gradient 0.06 -> 0.09 — the ramp data
        // itself ([0.01, 0.05] -> [0.01, 0.08]). The JOINT moved; the
        // stale downstream start is refused — the chain-level law a drag
        // cannot dodge, exactly the check the plugins' re-declares ride.
        let mut edited = vertical.clone();
        edited.segments[1].def.ramp = ramp_linear(0.01, 0.09);
        let err = audit_chain(&edited, 1.0e-3).unwrap_err();
        assert!(
            err.contains("seam segment 1 to 2"),
            "the stale joint refuses with the audited pair: {err}"
        );

        // The honest re-declare: the joint comes from sample_chain at the
        // edited front chain's end (s = 300) — the engine's own
        // evaluation, never a re-typed constant.
        let front = ParametricChain {
            plane: ParametricPlane::Profile,
            segments: edited.segments[..2].to_vec(),
            layer: String::new(),
        };
        let joint = sample_chain(&front, 300.0)
            .expect("the front samples")
            .expect("the joint sits at the edited front chain's end");
        assert!((joint.point[0] - 300.0).abs() < 1e-9);
        assert!(
            (joint.point[1] - 11.0).abs() < 1e-9,
            "the crest height moved to the new mean-gradient end ({})",
            joint.point[1]
        );
        let joint_gradient = joint.direction[1] / joint.direction[0];
        assert!(
            (joint_gradient - 0.09).abs() < 1e-9,
            "the joint slope restates the edited ramp end value"
        );
        edited.segments[2].def = profile_const_slope_def(80.0, joint_gradient);
        edited.segments[2].start_point = joint.point;
        edited.segments[2].start_direction = joint_gradient.atan();
        audit_chain(&edited, 1.0e-3).expect("the honest re-declare audits green");

        // The drag propagates the whole way: the tail's end rides the
        // moved joint (h = 11 + 0.09*80 = 18.2).
        let tail_end = sample_chain(&edited, 380.0).expect("the edited chain samples").unwrap();
        assert!(
            (tail_end.point[1] - (11.0 + 0.09 * 80.0)).abs() < 1e-9,
            "the edited tail's end follows the moved joint"
        );

        // The carrier path follows: three exact carriers again, and the
        // crest spline's last control point rides the new crest height.
        let (entities, carriers) =
            build_display_entities(&edited, 1e-3).expect("the edited crest stages");
        assert_eq!(entities.len(), 3);
        assert!(matches!(carriers[0], StagedCarrier::Exact));
        assert!(matches!(carriers[1], StagedCarrier::Exact));
        assert!(matches!(carriers[2], StagedCarrier::Exact));
        let EntityType::Spline(spline) = &entities[1] else {
            panic!("the crest stages an exact degree-2 spline");
        };
        assert_eq!(spline.degree, 2);
        let crest = &spline.control_points[2];
        assert!((crest.x - 300.0).abs() < 1e-9);
        assert!(
            (crest.y - 11.0).abs() < 1e-9,
            "the crest control point rides the edited height"
        );

        // ---- the plan mirror: the revised ramp end re-welds the tail ----
        let plan = demo_plan_chain();
        let mut edited = plan.clone();
        edited.segments[1].def.ramp = ramp_linear(0.0, 1.0 / -240.0);
        let front = ParametricChain {
            plane: ParametricPlane::Plan,
            segments: edited.segments[..2].to_vec(),
            layer: String::new(),
        };
        let joint = sample_chain(&front, 120.0)
            .expect("the front samples")
            .expect("the joint sits at the edited front chain's end");
        // The independent series oracle cross-checks the engine's own
        // integrator (the same law, restated from scratch).
        let (ox, oy) = spiral_pos((1.0 / -240.0) / (2.0 * 60.0), 60.0);
        assert!((joint.point[0] - (60.0 + ox)).abs() < 1e-9, "the joint x");
        assert!((joint.point[1] - oy).abs() < 1e-9, "the joint y");
        let realized = joint.direction[1].atan2(joint.direction[0]);
        assert!(
            (realized - (1.0 / -240.0) * 60.0 / 2.0).abs() < 1e-9,
            "the joint bearing restates the revised end curvature"
        );
        edited.segments[2].start_point = joint.point;
        edited.segments[2].start_direction = realized;
        audit_chain(&edited, 1.0e-3).expect("the honest plan re-declare audits green");
        let (_, carriers) = build_display_entities(&edited, 1e-3).expect("builds");
        assert!(matches!(carriers[1], StagedCarrier::Chords(_)));
    }

    // ==== caps, layers, round trips =======================================

    #[test]
    fn helix_never_stages_a_plan_ramp_curve() {
        // The contract's negative: a plan ramp curve is a 2D law; a Helix
        // is a 3D constant-pitch space spiral. The staged carrier set only
        // ever contains the four supported types.
        let chain = demo_plan_chain();
        let (entities, _) = build_display_entities(&chain, 1e-3).unwrap();
        for entity in &entities {
            assert!(
                matches!(
                    entity,
                    EntityType::Line(_)
                        | EntityType::Arc(_)
                        | EntityType::LwPolyline(_)
                        | EntityType::Spline(_)
                ),
                "only line/arc/spline/polyline carriers exist in this contract"
            );
        }
    }

    #[test]
    fn cap_refuses_over_budget_chains() {
        // A 50-segment chain (short lines welded to spiral ramps, seams
        // exact by construction) at a 1e-4 tolerance: the chord plans sum
        // far above the live cap, so nothing publishes.
        let mut segments = Vec::new();
        let mut cursor = [0.0f64, 0.0];
        let mut heading = 0.0f64;
        for _ in 0..25 {
            segments.push(segment_of(ParametricPlane::Plan, plan_line_def(2.0), cursor, heading));
            cursor = [
                cursor[0] + 2.0 * heading.cos(),
                cursor[1] + 2.0 * heading.sin(),
            ];
            segments.push(segment_of(
                ParametricPlane::Plan,
                plan_spiral_def(60.0, 1.0 / -300.0),
                cursor,
                heading,
            ));
            let one = ParametricChain {
                plane: ParametricPlane::Plan,
                segments: vec![segments.last().unwrap().clone()],
                layer: String::new(),
            };
            let vitals = audit_chain(&one, 1.0e-3).unwrap();
            let end = evaluate_segment_at(ParametricPlane::Plan, &one.segments[0], &vitals[0], 60.0);
            cursor = end.point;
            heading += 60.0 / (2.0 * -300.0);
        }
        let chain = ParametricChain {
            plane: ParametricPlane::Plan,
            segments,
            layer: String::new(),
        };
        audit_chain(&chain, 1.0e-3).expect("the welded 50-segment chain audits");
        let result = build_display_entities(&chain, 1e-4);
        let err = result.err().expect("the chain's footprint is over the cap");
        assert!(err.contains("MAX_PLUGIN_LIVE_ENTITIES"), "{err}");
        assert!(err.contains("nothing staged"), "{err}");
    }

    #[test]
    fn entity_layers_ride_the_chain_payload() {
        let chain = demo_plan_chain();
        let (entities, _) = build_display_entities(&chain, 1e-3).unwrap();
        for entity in &entities {
            assert_eq!(entity.common().layer, "PLUGIN_DISPLAY");
        }
        let mut chain = demo_plan_chain();
        chain.layer = String::new();
        let (entities, _) = build_display_entities(&chain, 1e-3).unwrap();
        for entity in &entities {
            // An empty layer name keeps the codec default.
            assert_eq!(entity.common().layer, "0");
        }
    }

    #[test]
    fn declared_chain_and_xrecords_survive_dxf_and_dwg_round_trips() {
        // THE entity contract of this surface: the staged display entities
        // ride the shared drawing model, and the record model's XRecords —
        // the plugins' declared truth — survive the SAME file round trip.
        // Loss reports must stay empty for both formats.
        use codec::Format;

        // 1) Declare + stage.
        let plan = demo_plan_chain();
        let profile = demo_profile_chain();
        let mut all_entities = Vec::new();
        all_entities.extend(build_display_entities(&plan, 1e-3).unwrap().0);
        all_entities.extend(build_display_entities(&profile, 1e-3).unwrap().0);
        // One staged entity carries the record model's XDATA (the
        // placement class the entity-bound records ride in).
        if let EntityType::LwPolyline(polyline) = &mut all_entities[1] {
            let mut record = codec::xdata::ExtendedDataRecord::new("PLUGIN");
            record
                .add_value(codec::xdata::XDataValue::String("alignment display carrier".into()));
            record.add_value(codec::xdata::XDataValue::Real(104.0));
            polyline.common.extended_data.add_record(record);
        }

        // 2) A document + the staged set + the record model's XRecords.
        let mut document = codec::Document::new();
        // 2b) The display layers are registered entries of the document's
        // layer table (the plugin consumer creates its display layers
        // before staging; an unregistered layer name falls back to "0" on
        // read, by the model's own contract).
        document
            .layers
            .add(codec::tables::Layer::new("PLUGIN_DISPLAY"))
            .unwrap();
        document
            .layers
            .add(codec::tables::Layer::new("PLUGIN_PROFILE"))
            .unwrap();
        let mut staged_handles = Vec::new();
        for entity in all_entities {
            staged_handles.push(document.add_entity(entity).unwrap());
        }
        let appid = codec::tables::AppId::new("PLUGIN");
        document.app_ids.add(appid).unwrap();
        let nod_handle = document.header.named_objects_dict_handle;
        let dict_handle = document.allocate_handle();
        let record_handle = document.allocate_handle();
        let mut record = codec::objects::XRecord::named("PLUGIN_ALIGN_DISPLAY");
        record.handle = record_handle;
        record.owner = dict_handle;
        record.entries = vec![
            codec::objects::XRecordEntry::string(1, "parametric chain v14"),
            codec::objects::XRecordEntry::double(140, 104.0),
            codec::objects::XRecordEntry::int32(420, 7),
        ];
        document
            .objects
            .insert(record_handle, codec::objects::ObjectType::XRecord(record));
        let mut dict = codec::objects::Dictionary::new();
        dict.handle = dict_handle;
        dict.owner = nod_handle;
        dict.hard_owner = true;
        // The record's name and its dictionary key are ONE name (the DWG
        // canonical semantics: the XRecord's identity rides the owning
        // dictionary).
        dict.add_entry("PLUGIN_ALIGN_DISPLAY", record_handle);
        document
            .objects
            .insert(dict_handle, codec::objects::ObjectType::Dictionary(dict));
        if let Some(codec::objects::ObjectType::Dictionary(nod)) = document.objects.get_mut(&nod_handle)
        {
            nod.add_entry("PLUGIN", dict_handle);
        }

        // 3) Round trips.
        let original_record =
            document.objects.get(&record_handle).cloned().expect("record present");
        let original_kind_of =
            |h: codec::Handle| document.get_entity(h).map(|e| std::mem::discriminant(e));
        let expected_kinds: Vec<_> = staged_handles
            .iter()
            .map(|h| original_kind_of(*h).expect("handle present"))
            .collect();

        let (dxf_bytes, dxf_losses) = codec::Dxf.write(&document).unwrap();
        assert!(
            dxf_losses.is_empty(),
            "DXF write must not lose anything: {dxf_losses:?}"
        );
        let (dxf_doc, dxf_read_losses) = codec::Dxf.read(&dxf_bytes).unwrap();
        assert!(
            dxf_read_losses.is_empty(),
            "DXF read must not lose anything: {dxf_read_losses:?}"
        );
        assert_staged(&dxf_doc, &staged_handles, &expected_kinds);
        assert_record_model(&dxf_doc, &original_record);

        let (dwg_bytes, dwg_losses) = codec::Dwg.write(&document).unwrap();
        assert!(
            dwg_losses.is_empty(),
            "DWG write must not lose anything: {dwg_losses:?}"
        );
        let (dwg_doc, dwg_read_losses) = codec::Dwg.read(&dwg_bytes).unwrap();
        assert!(
            dwg_read_losses.is_empty(),
            "DWG read must not lose anything: {dwg_read_losses:?}"
        );
        assert_staged(&dwg_doc, &staged_handles, &expected_kinds);
        assert_record_model(&dwg_doc, &original_record);

        // 4) The entity-bound record rides too: the staged polyline's
        // XDATA (the PLUGIN app record) survives, values verbatim.
        let assert_xdata = |document: &codec::Document, stage: &str| {
            let entity = document
                .get_entity(staged_handles[1])
                .expect("staged polyline present");
            if let EntityType::LwPolyline(polyline) = entity {
                let record = polyline
                    .common
                    .extended_data
                    .get_record("PLUGIN")
                    .unwrap_or_else(|| panic!("{stage}: the PLUGIN XDATA lost"));
                assert_eq!(record.values.len(), 2);
            } else {
                panic!("{stage}: entity 1 became another type");
            }
        };
        assert_xdata(&dxf_doc, "DXF");
        assert_xdata(&dwg_doc, "DWG");

        // 5) The second hop (file -> document -> file) must be stationary.
        let (dxf_bytes2, dxf_losses2) = codec::Dxf.write(&dxf_doc).unwrap();
        assert!(dxf_losses2.is_empty());
        let (dxf_doc2, _) = codec::Dxf.read(&dxf_bytes2).unwrap();
        assert_record_model(&dxf_doc2, &original_record);
        assert_staged(&dxf_doc2, &staged_handles, &expected_kinds);

        fn assert_staged(
            document: &codec::Document,
            staged: &[codec::Handle],
            kinds: &[std::mem::Discriminant<codec::EntityType>],
        ) {
            for (i, (handle, kind)) in staged.iter().zip(kinds.iter()).enumerate() {
                let entity = document
                    .get_entity(*handle)
                    .unwrap_or_else(|| panic!("staged entity {handle:?} lost in the round trip"));
                assert_eq!(std::mem::discriminant(entity), *kind, "type drifted");
                // The chain's layer attribution survives the file hop: the
                // plan stages on PLUGIN_DISPLAY, the profile on
                // PLUGIN_PROFILE.
                let expected = if i < 3 { "PLUGIN_DISPLAY" } else { "PLUGIN_PROFILE" };
                assert_eq!(entity.common().layer, expected, "layer attribution lost");
            }
        }

        fn assert_record_model(document: &codec::Document, original: &codec::objects::ObjectType) {
            // The XRecord's entries must survive BYTE-IDENTICAL, alongside
            // its dictionary and the NOD linkage (record + display live and
            // die together in the same file).
            let codec::objects::ObjectType::XRecord(original_record) = original else {
                panic!("expected an XRecord");
            };
            let read = document
                .objects
                .get(&original_record.handle)
                .unwrap_or_else(|| panic!("XRecord lost in the round trip"));
            let codec::objects::ObjectType::XRecord(read_record) = read else {
                panic!("XRecord became another object");
            };
            assert_eq!(
                read_record.entries, original_record.entries,
                "entries must be byte-identical"
            );
            assert_eq!(read_record.name, original_record.name);
            // The extension dictionary survived with its entries.
            let dict_read = document
                .objects
                .get(&original_record.owner)
                .expect("the record's dictionary survived");
            let codec::objects::ObjectType::Dictionary(dict) = dict_read else {
                panic!("dictionary became another object");
            };
            assert!(
                dict.entries.iter().any(|(name, _)| name == "PLUGIN_ALIGN_DISPLAY"),
                "the dictionary entry survived"
            );
        }
    }
}
