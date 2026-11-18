//! Exact mass properties for analytic closed solids.

use super::{Body, Curve3, Surface};
use ocs_geom::space::Vec3;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProperties {
    pub volume: f64,
    pub centroid: [f64; 3],
    pub moment_of_inertia: [f64; 3],
    pub principal_directions: [f64; 9],
    pub principal_moments: [f64; 3],
    pub product_of_inertia: [f64; 3],
    pub radii_of_gyration: [f64; 3],
}

impl MassProperties {
    /// Return the same body translated in world space.
    pub fn translated(mut self, delta: [f64; 3]) -> Self {
        let old = self.centroid;
        let new = [old[0] + delta[0], old[1] + delta[1], old[2] + delta[2]];
        let square_change = |axis| delta[axis] * (2.0 * old[axis] + delta[axis]);
        self.moment_of_inertia[0] += self.volume * (square_change(1) + square_change(2));
        self.moment_of_inertia[1] += self.volume * (square_change(0) + square_change(2));
        self.moment_of_inertia[2] += self.volume * (square_change(0) + square_change(1));
        let product_change = |first, second| {
            old[first] * delta[second] + old[second] * delta[first] + delta[first] * delta[second]
        };
        self.product_of_inertia[0] -= self.volume * product_change(0, 1);
        self.product_of_inertia[1] -= self.volume * product_change(1, 2);
        self.product_of_inertia[2] -= self.volume * product_change(2, 0);
        self.radii_of_gyration = self
            .moment_of_inertia
            .map(|moment| (moment.max(0.0) / self.volume).sqrt());
        self.centroid = new;
        self
    }
}

/// Exact properties for complete analytic spheres and circular cylinders,
/// including the concentric closed shells produced by [`super::shell`].
pub fn analytic_mass_properties(body: &Body) -> Option<MassProperties> {
    sphere_properties(body)
        .or_else(|| cylindrical_sector_properties(body))
        .or_else(|| cylinder_properties(body))
}

/// Mass properties of any closed body: exact where
/// [`analytic_mass_properties`] knows the shape, then integrated over the
/// faces themselves to integration tolerance ([`integrated_mass_properties`]),
/// and otherwise over a mesh. An inscribed mesh falls short of a curved face
/// by the square of its step, so two meshes a halving apart extrapolate to
/// the surface itself, to a few parts in ten million. `None` when the body
/// cannot be meshed whole.
pub fn mass_properties(body: &Body) -> Option<MassProperties> {
    if let Some(properties) = analytic_mass_properties(body) {
        return Some(properties);
    }
    if let Some(properties) = integrated_mass_properties(body) {
        return Some(properties);
    }
    // Solidcore's integrator for what this one cannot trace; the mesh last.
    if let Some(properties) = solidcore_mass_properties(body) {
        return Some(properties);
    }
    meshed_mass_properties(body)
}

/// [`mass_properties`] by Solidcore's integrator.
pub fn solidcore_mass_properties(body: &Body) -> Option<MassProperties> {
    let (sc, _, _) = super::bridge::to_solidcore(body).ok()?;
    let m = sc.mass_properties(1e-9).ok()?;
    if !(m.volume.is_finite() && m.volume > 0.0) {
        return None;
    }
    let (volume, centroid, central) = (m.volume, m.centroid, m.tensor);
    let (principal_moments, principal_directions) = super::mesh::principal_axes(central);
    let c2 = centroid.iter().map(|value| value * value).sum::<f64>();
    let origin: [[f64; 3]; 3] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            central[row][column]
                + volume * (if row == column { c2 } else { 0.0 } - centroid[row] * centroid[column])
        })
    });
    let moment_of_inertia = [origin[0][0], origin[1][1], origin[2][2]];
    Some(MassProperties {
        volume,
        centroid,
        moment_of_inertia,
        principal_directions,
        principal_moments,
        product_of_inertia: [origin[0][1], origin[1][2], origin[2][0]],
        radii_of_gyration: moment_of_inertia.map(|moment| (moment.max(0.0) / volume).sqrt()),
    })
}

/// [`mass_properties`] by the mesh alone.
pub fn meshed_mass_properties(body: &Body) -> Option<MassProperties> {
    let points: Vec<[f64; 3]> = body.vertices.iter().map(|(_, vertex)| vertex.point).collect();
    let size = super::bounds::around_points(&points)
        .map(|bounds| Vec3::from(bounds.min).distance(Vec3::from(bounds.max)))
        .filter(|size| *size > 0.0)
        .unwrap_or(1.0);
    // The chord bound shrinks with the square of the angle, as the sag of a
    // step does, so both meshes are fined down by the same rule.
    let meshed = |angle: f64| {
        let chord = size * 1e-5 * (angle / 0.05).powi(2);
        let mesh = super::mesh::tessellate(
            body,
            super::mesh::TessellationTolerance::new(angle, chord),
        );
        mesh.missing_faces
            .is_empty()
            .then(|| Some((mesh.mesh.inertial_properties()?, mesh.crossed)))
            .flatten()
    };
    let (fine, crossed) = meshed(0.05).or_else(|| meshed(0.2))?;
    // A face filled across touching rings converges unevenly: measured finer
    // rather than extrapolated.
    if crossed {
        return Some(meshed(0.0125).map_or(fine, |(finest, _)| finest));
    }
    let meshed = |angle: f64| meshed(angle).map(|(properties, _)| properties);
    // A face too fine to triangulate whole at the finer step still has its
    // properties at the coarser one.
    let Some(coarse) = meshed(0.1) else {
        return Some(fine);
    };
    // Two steps a halving apart differ by a few parts in ten thousand. More
    // than that, one of them is off — a spline tube meshed over itself at
    // the coarser step — and the pair a step finer settles it.
    let apart = (fine.volume - coarse.volume).abs() > 2e-3 * fine.volume.abs();
    if apart {
        if let Some(finest) = meshed(0.025) {
            return Some(extrapolated(&finest, &fine).unwrap_or(finest));
        }
    }
    Some(extrapolated(&fine, &coarse).unwrap_or(fine))
}

/// Properties from a mesh and one twice as coarse, each integral carried on
/// past the finer by a third of what the halving changed.
fn extrapolated(fine: &MassProperties, coarse: &MassProperties) -> Option<MassProperties> {
    let mix = |fine: f64, coarse: f64| fine + (fine - coarse) / 3.0;
    let volume = mix(fine.volume, coarse.volume);
    if !volume.is_finite() || volume <= 0.0 {
        return None;
    }
    let centroid: [f64; 3] = std::array::from_fn(|axis| {
        mix(fine.volume * fine.centroid[axis], coarse.volume * coarse.centroid[axis]) / volume
    });
    // About the origin, where each entry is an integral of its own.
    let tensor = |properties: &MassProperties| {
        let [xx, yy, zz] = properties.moment_of_inertia;
        let [xy, yz, zx] = properties.product_of_inertia;
        [[xx, xy, zx], [xy, yy, yz], [zx, yz, zz]]
    };
    let (fine_tensor, coarse_tensor) = (tensor(fine), tensor(coarse));
    let origin: [[f64; 3]; 3] = std::array::from_fn(|row| {
        std::array::from_fn(|column| mix(fine_tensor[row][column], coarse_tensor[row][column]))
    });
    let c2 = centroid.iter().map(|value| value * value).sum::<f64>();
    let central: [[f64; 3]; 3] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            let diagonal = if row == column { c2 } else { 0.0 };
            origin[row][column] - volume * (diagonal - centroid[row] * centroid[column])
        })
    });
    let (principal_moments, principal_directions) = super::mesh::principal_axes(central);
    let moment_of_inertia = [origin[0][0], origin[1][1], origin[2][2]];
    let properties = MassProperties {
        volume,
        centroid,
        moment_of_inertia,
        principal_directions,
        principal_moments,
        product_of_inertia: [origin[0][1], origin[1][2], origin[2][0]],
        radii_of_gyration: moment_of_inertia.map(|moment| (moment.max(0.0) / volume).sqrt()),
    };
    centroid
        .iter()
        .chain(&properties.moment_of_inertia)
        .chain(&properties.principal_moments)
        .all(|value| value.is_finite())
        .then_some(properties)
}

// ---------------------------------------------------------------------------
// Integration over the faces themselves.
//
// Ported from Solidcore's `mass/integrator/properties.rs` (NVIDIA,
// Apache-2.0): the volume integrals are fluxes of fields `(0, 0, g)` through
// the boundary, and each face's surface integral becomes, by Green's
// theorem, a loop integral round its trims of an integral along iso-`u`
// lines, both by adaptive Gauss–Kronrod quadrature split at knots. Here the
// trims are traced along the edges' own space curves and taken into the
// surface's parameters point by point, so a trim stored only as a sampled
// polyline costs nothing in accuracy.
// ---------------------------------------------------------------------------

/// The 15-point Gauss–Kronrod rule on `[-1, 1]`: the nodes `0, ±x1, ...,
/// ±x7`, their Kronrod weights, and the weights of the embedded 7-point Gauss
/// rule (zero off its nodes). The Kronrod rule is exact to degree 22.
const KRONROD15_NODES: [f64; 8] = [
    0.0,
    0.207_784_955_007_898_467_600_689_403_773_245,
    0.405_845_151_377_397_166_906_606_412_076_961,
    0.586_087_235_467_691_130_294_144_845_693_013,
    0.741_531_185_599_394_439_863_864_773_280_788,
    0.864_864_423_359_769_072_789_712_788_640_926,
    0.949_107_912_342_758_524_526_189_684_047_851,
    0.991_455_371_120_812_639_206_854_697_526_329,
];
const KRONROD15_WEIGHTS: [f64; 8] = [
    0.209_482_141_084_727_828_012_999_174_891_714,
    0.204_432_940_075_298_892_414_161_999_234_649,
    0.190_350_578_064_785_409_913_256_402_421_014,
    0.169_004_726_639_267_902_826_583_426_598_550,
    0.140_653_259_715_525_918_745_189_590_510_238,
    0.104_790_010_322_250_183_839_876_322_541_518,
    0.063_092_092_629_978_553_290_700_663_189_204,
    0.022_935_322_010_529_224_963_732_008_058_970,
];
const GAUSS7_WEIGHTS: [f64; 8] = [
    0.417_959_183_673_469_387_755_102_040_816_327,
    0.0,
    0.381_830_050_505_118_944_950_369_775_488_975,
    0.0,
    0.279_705_391_489_276_667_901_467_771_423_780,
    0.0,
    0.129_484_966_168_869_693_270_611_432_679_082,
    0.0,
];
/// The deepest bisection, and the most pieces, one range is cut into.
const KRONROD_MAX_DEPTH: u32 = 48;
const KRONROD_MAX_PIECES: u32 = 100_000;

/// The integral of `integrand` from `a` to `b` by adaptive 15-point
/// Gauss–Kronrod quadrature. A piece is kept when its Kronrod and Gauss
/// values differ, in every component, by no more than `accuracy` times the
/// larger of its largest component and `floor` times its share of the range;
/// otherwise it is halved. `floor` is what the whole range's integral is
/// measured against when it is near zero.
///
/// Each rule is summed about the centre node's value, so a constant
/// integrand gives exactly the width times it. `None` when the integrand
/// fails or is not finite, or the range will not settle.
pub(crate) fn kronrod<const N: usize>(
    a: f64,
    b: f64,
    accuracy: f64,
    floor: f64,
    mut integrand: impl FnMut(f64) -> Option<[f64; N]>,
) -> Option<[f64; N]> {
    let mut total = [0.0; N];
    if a == b {
        return Some(total);
    }
    let whole = (b - a).abs();
    let mut stack = vec![(a, b, 0_u32)];
    let mut pieces = 0_u32;
    while let Some((lo, hi, depth)) = stack.pop() {
        pieces += 1;
        if pieces > KRONROD_MAX_PIECES {
            return None;
        }
        let mid = 0.5 * (lo + hi);
        let half = 0.5 * (hi - lo);
        let centre = integrand(mid)?;
        let mut kronrod = [0.0; N];
        let mut gauss = [0.0; N];
        for (index, &x) in KRONROD15_NODES.iter().enumerate().skip(1) {
            for side in [-1.0, 1.0] {
                let value = integrand(mid + side * x * half)?;
                for component in 0..N {
                    let off = value[component] - centre[component];
                    kronrod[component] += KRONROD15_WEIGHTS[index] * off;
                    gauss[component] += GAUSS7_WEIGHTS[index] * off;
                }
            }
        }
        let mut largest = 0.0_f64;
        let mut error = 0.0_f64;
        for component in 0..N {
            kronrod[component] = half * (2.0 * centre[component] + kronrod[component]);
            gauss[component] = half * (2.0 * centre[component] + gauss[component]);
            largest = largest.max(kronrod[component].abs());
            error = error.max((kronrod[component] - gauss[component]).abs());
        }
        if !error.is_finite() || !largest.is_finite() {
            return None;
        }
        if error <= accuracy * largest.max(floor * (hi - lo).abs() / whole) {
            for component in 0..N {
                total[component] += kronrod[component];
            }
            continue;
        }
        if depth >= KRONROD_MAX_DEPTH || mid == lo || mid == hi {
            return None;
        }
        stack.push((mid, hi, depth + 1));
        stack.push((lo, mid, depth + 1));
    }
    Some(total)
}

/// [`kronrod`] over `a..b` split at `cuts` (those strictly between, in
/// order of the run).
fn kronrod_split<const N: usize>(
    a: f64,
    b: f64,
    cuts: &[f64],
    accuracy: f64,
    floor: f64,
    mut integrand: impl FnMut(f64) -> Option<[f64; N]>,
) -> Option<[f64; N]> {
    let (low, high) = (a.min(b), a.max(b));
    let mut inside: Vec<f64> = cuts.iter().copied().filter(|cut| *cut > low && *cut < high).collect();
    inside.sort_by(f64::total_cmp);
    inside.dedup();
    if b < a {
        inside.reverse();
    }
    let whole = (b - a).abs();
    let mut total = [0.0; N];
    let mut from = a;
    for to in inside.into_iter().chain(std::iter::once(b)) {
        let share = floor * (to - from).abs() / whole;
        let piece = kronrod(from, to, accuracy, share, &mut integrand)?;
        for component in 0..N {
            total[component] += piece[component];
        }
        from = to;
    }
    Some(total)
}

/// How tightly each level of the nested integrals is held, as a fraction of
/// the body's own scale.
const ACCURACY: f64 = 1e-11;

/// The integrals [volume, `x`, `y`, `z`, `x²`, `y²`, `z²`, `xy`, `yz`,
/// `zx`] over a body's volume, in coordinates centred on and scaled by the
/// body's box.
type Moments = [f64; 10];

/// A face's surface seen in the order its parameters are integrated: `a`
/// along the trims and `b` up each iso-`a` line. Swapped, `a` is the
/// surface's `v` — for a face whose loops wrap round `v` instead of `u`.
struct FaceMap<'a> {
    surface: &'a Surface,
    swap: bool,
    centre: Vec3,
    scale: f64,
}

impl FaceMap<'_> {
    fn uv(&self, ab: [f64; 2]) -> (f64, f64) {
        if self.swap {
            (ab[1], ab[0])
        } else {
            (ab[0], ab[1])
        }
    }

    fn ab(&self, (u, v): (f64, f64)) -> [f64; 2] {
        if self.swap {
            [v, u]
        } else {
            [u, v]
        }
    }

    fn periods(&self) -> [Option<f64>; 2] {
        let [u, v] = super::pcurve::periods(self.surface);
        if self.swap {
            [v, u]
        } else {
            [u, v]
        }
    }

    fn point(&self, ab: [f64; 2]) -> Vec3 {
        let (u, v) = self.uv(ab);
        Vec3::from(self.surface.point_at(u, v))
    }

    /// The tangents along `a` and `b`.
    fn tangents(&self, ab: [f64; 2]) -> Option<(Vec3, Vec3)> {
        let (u, v) = self.uv(ab);
        let (along_u, along_v) = self.surface.tangents_at(u, v)?;
        let (along_u, along_v) = (Vec3::from(along_u), Vec3::from(along_v));
        Some(if self.swap { (along_v, along_u) } else { (along_u, along_v) })
    }

    /// The flux integrands at `ab` per unit `a` and `b`: each of [`Moments`]
    /// as `∂g/∂z` of a field `(0, 0, g)`, times the `z` part of `Sa × Sb`.
    fn integrand(&self, ab: [f64; 2]) -> Option<Moments> {
        let p = (self.point(ab) - self.centre) / self.scale;
        let (along_a, along_b) = self.tangents(ab)?;
        let n = along_a.cross(along_b).z / (self.scale * self.scale);
        let (x, y, z) = (p.x, p.y, p.z);
        let values = [
            z,
            x * z,
            y * z,
            z * z / 2.0,
            x * x * z,
            y * y * z,
            z * z * z / 3.0,
            x * y * z,
            y * z * z / 2.0,
            x * z * z / 2.0,
        ];
        let values = values.map(|value| value * n);
        values.iter().all(|value| value.is_finite()).then_some(values)
    }

    /// The knots across which the surface is not smooth along `b`.
    fn b_knots(&self) -> Vec<f64> {
        let Some(spline) = self.surface.spline() else {
            return Vec::new();
        };
        let (u_knots, v_knots) = spline.knots();
        if self.swap {
            return u_knots.to_vec();
        }
        if spline.v_reversed() {
            let (_, (v0, v1)) = spline.domain();
            return v_knots.iter().map(|knot| v0 + v1 - knot).collect();
        }
        v_knots.to_vec()
    }

    /// `∫ f(a, s) ds` from `base` to `b`, held to `accuracy` against `floor`.
    fn iso_integral(&self, ab: [f64; 2], base: f64, knots: &[f64], floor: f64) -> Option<Moments> {
        let [a, b] = ab;
        kronrod_split(base, b, knots, ACCURACY, floor, |s| self.integrand([a, s]))
    }

    /// The `(a, b)` of a point on the surface, nearest `near` where a
    /// parameter wraps.
    fn invert(&self, point: [f64; 3], near: [f64; 2]) -> Option<[f64; 2]> {
        let raw = if self.surface.is_spline() {
            // Newton's method from the neighbouring sample: a spline's own
            // inverse starts from a grid search on every call.
            self.refine(point, near)
                .or_else(|| self.surface.parameters_at(point).map(|uv| self.ab(uv)))?
        } else {
            self.ab(self.surface.parameters_at(point)?)
        };
        let periods = self.periods();
        Some(std::array::from_fn(|axis| unwrap(raw[axis], near[axis], periods[axis])))
    }

    /// `ab` held inside a spline's domain where it does not wrap: past it the
    /// evaluation clamps, and a point on the domain's edge would let the
    /// parameter drift outward unchecked.
    fn clamp(&self, ab: [f64; 2]) -> [f64; 2] {
        let Some(spline) = self.surface.spline() else {
            return ab;
        };
        let (u, v) = spline.domain();
        let ranges = if self.swap { [v, u] } else { [u, v] };
        let periods = self.periods();
        std::array::from_fn(|axis| match periods[axis] {
            Some(_) => ab[axis],
            None => ab[axis].clamp(ranges[axis].0, ranges[axis].1),
        })
    }

    fn refine(&self, point: [f64; 3], seed: [f64; 2]) -> Option<[f64; 2]> {
        let target = Vec3::from(point);
        let mut ab = seed;
        for _ in 0..24 {
            let residual = self.point(ab) - target;
            let (along_a, along_b) = self.tangents(ab)?;
            let (aa, ab_, bb) = (along_a.dot(along_a), along_a.dot(along_b), along_b.dot(along_b));
            let determinant = aa * bb - ab_ * ab_;
            if !determinant.is_finite() || determinant <= 1e-24 * aa * bb {
                return None;
            }
            let (ra, rb) = (along_a.dot(residual), along_b.dot(residual));
            let step = [(-ra * bb + ab_ * rb) / determinant, (ab_ * ra - aa * rb) / determinant];
            ab = self.clamp([ab[0] + step[0], ab[1] + step[1]]);
            if step[0].abs() + step[1].abs() <= 1e-14 * (1.0 + ab[0].abs() + ab[1].abs()) {
                let off = (self.point(ab) - target).length();
                return (off <= 1e-7 * self.scale).then_some(ab);
            }
        }
        None
    }
}

fn unwrap(value: f64, near: f64, period: Option<f64>) -> f64 {
    match period.filter(|period| period.is_finite() && *period > 0.0) {
        Some(period) => value + period * ((near - value) / period).round(),
        None => value,
    }
}

/// The velocity of an edge's curve at `t`.
fn velocity(curve: &Curve3, t: f64) -> Vec3 {
    Vec3::from(match curve {
        Curve3::Nurbs(curve) => curve.derivative_at_knot(t),
        _ => curve.tangent_at(t),
    })
}

/// How many samples a trace keeps between its ends.
const TRACE_SAMPLES: usize = 64;

/// One coedge's trim, followed along its edge's space curve.
struct Trace<'a> {
    curve: &'a Curve3,
    /// The curve parameters the coedge runs between, in the loop's
    /// direction.
    from: f64,
    to: f64,
    /// `(a, b)` at evenly spaced parameters, the ends a hair inside, unwound
    /// to run continuously.
    samples: Vec<[f64; 2]>,
    /// Where the trim starts and ends.
    start: [f64; 2],
    end: [f64; 2],
}

impl Trace<'_> {
    fn shift(&mut self, by: [f64; 2]) {
        for sample in self.samples.iter_mut().chain([&mut self.start, &mut self.end]) {
            sample[0] += by[0];
            sample[1] += by[1];
        }
    }

    fn at(&self, map: &FaceMap<'_>, t: f64) -> Option<[f64; 2]> {
        let share = ((t - self.from) / (self.to - self.from)).clamp(0.0, 1.0);
        let index = (share * TRACE_SAMPLES as f64).round() as usize;
        map.invert(self.curve.point_at(t), self.samples[index.min(TRACE_SAMPLES)])
    }

    /// The curve's knots between the ends, where the trim may bend sharply.
    fn knots(&self) -> Vec<f64> {
        match self.curve {
            Curve3::Nurbs(curve) => curve.knots().to_vec(),
            _ => Vec::new(),
        }
    }
}

fn trace<'a>(
    map: &FaceMap<'_>,
    curve: &'a Curve3,
    from: f64,
    to: f64,
    anchor: Option<[f64; 2]>,
) -> Option<Trace<'a>> {
    let periods = map.periods();
    let span = to - from;
    let nudge = 1e-9;
    let mut samples: Vec<[f64; 2]> = Vec::with_capacity(TRACE_SAMPLES + 1);
    for index in 0..=TRACE_SAMPLES {
        let share = (index as f64 / TRACE_SAMPLES as f64).clamp(nudge, 1.0 - nudge);
        let point = curve.point_at(from + span * share);
        let near = match samples.last() {
            Some(previous) => *previous,
            None => match anchor {
                Some(anchor) => anchor,
                None => map.ab(map.surface.parameters_at(point)?),
            },
        };
        let at = map.invert(point, near)?;
        // A trim that jumps between neighbouring samples runs through a
        // pole, where it leaves the surface's parameters in two places.
        if let Some(previous) = samples.last() {
            for axis in 0..2 {
                if let Some(period) = periods[axis] {
                    if (at[axis] - previous[axis]).abs() > period / 4.0 {
                        return None;
                    }
                }
            }
        }
        samples.push(at);
    }
    // The ends themselves where they invert cleanly; a pole or an apex
    // answers for one longitude only, and the nudged sample stands there.
    let end_at = |parameter: f64, nudged: [f64; 2]| {
        map.invert(curve.point_at(parameter), nudged)
            .filter(|exact| {
                (0..2).all(|axis| (exact[axis] - nudged[axis]).abs() <= 1e-6 * (1.0 + nudged[axis].abs()))
            })
            .unwrap_or(nudged)
    };
    let start = end_at(from, samples[0]);
    let end = end_at(to, samples[TRACE_SAMPLES]);
    Some(Trace { curve, from, to, samples, start, end })
}

/// `-∫ G da` along one trim, `G` the iso integral up from `base`.
fn trace_integral(map: &FaceMap<'_>, trace: &Trace<'_>, base: f64, knots: &[f64]) -> Option<Moments> {
    let length = (trace.to - trace.from).abs();
    let along = trace.knots();
    kronrod_split(trace.from, trace.to, &along, ACCURACY, 1.0, |t| {
        let ab = trace.at(map, t)?;
        let (along_a, along_b) = map.tangents(ab)?;
        let speed = velocity(trace.curve, t);
        // The trim's own `da/dt`, from the space velocity resolved onto the
        // surface's tangents.
        let (aa, ab_, bb) = (along_a.dot(along_a), along_a.dot(along_b), along_b.dot(along_b));
        let determinant = aa * bb - ab_ * ab_;
        if !(determinant.is_finite() && determinant > 0.0) {
            return Some([0.0; 10]);
        }
        let da = (speed.dot(along_a) * bb - speed.dot(along_b) * ab_) / determinant;
        if da == 0.0 || !da.is_finite() {
            return Some([0.0; 10]);
        }
        let inner = map.iso_integral(ab, base, knots, 1.0 / (da.abs() * length))?;
        Some(inner.map(|value| -value * da))
    })
}

/// `-∫ G da` along the straight parameter-space segment between two trims.
fn gap_integral(map: &FaceMap<'_>, from: [f64; 2], to: [f64; 2], base: f64, knots: &[f64]) -> Option<Moments> {
    let da = to[0] - from[0];
    if da == 0.0 {
        return Some([0.0; 10]);
    }
    kronrod(0.0, 1.0, ACCURACY, 1.0, |s| {
        let ab = [from[0] + s * da, from[1] + s * (to[1] - from[1])];
        let inner = map.iso_integral(ab, base, knots, 1.0 / da.abs())?;
        Some(inner.map(|value| -value * da))
    })
}

/// Whether the parameter-space segment between two trims is one point in
/// space — the line along a pole or an apex, which bounds the face in its
/// parameters though it is no edge.
fn degenerate(map: &FaceMap<'_>, from: [f64; 2], to: [f64; 2]) -> bool {
    // Trims that simply meet are joined, not run along anything.
    if (to[0] - from[0]).abs() + (to[1] - from[1]).abs() <= 1e-9 * (1.0 + from[0].abs() + from[1].abs()) {
        return false;
    }
    let start = map.point(from);
    [0.25, 0.5, 0.75, 1.0].iter().all(|share| {
        let at = [from[0] + share * (to[0] - from[0]), from[1] + share * (to[1] - from[1])];
        (map.point(at) - start).length() <= 1e-6 * map.scale
    })
}

/// One loop's trims, joined where they meet across a period or a pole, and
/// how many times the loop winds round each wrapped parameter.
struct Ring<'a> {
    traces: Vec<Trace<'a>>,
    /// Segments closing the gaps between consecutive trims, and whether
    /// each runs along a pole.
    gaps: Vec<([f64; 2], [f64; 2], bool)>,
    winding: [i64; 2],
}

fn ring<'a>(
    body: &'a Body,
    map: &FaceMap<'_>,
    coedges: &[super::topology::CoedgeKey],
    anchors: &[Option<[f64; 2]>],
) -> Option<Ring<'a>> {
    let periods = map.periods();
    let mut traces: Vec<Trace<'a>> = Vec::with_capacity(coedges.len());
    let mut gaps = Vec::new();
    for (index, coedge) in coedges.iter().enumerate() {
        let coedge = body.coedges.get(*coedge)?;
        let edge = body.edges.get(coedge.edge)?;
        let curve = body.curves.get(edge.curve)?;
        let (from, to) = if coedge.forward {
            (edge.start_parameter, edge.end_parameter)
        } else {
            (edge.end_parameter, edge.start_parameter)
        };
        let mut next = trace(map, curve, from, to, anchors.get(index).copied().flatten())?;
        if let Some(previous) = traces.last() {
            let end = previous.end;
            // Across a regular point the two trims meet a whole number of
            // turns apart; across a pole they meet along it.
            if !degenerate(map, end, next.start) {
                let shift: [f64; 2] = std::array::from_fn(|axis| {
                    unwrap(next.start[axis], end[axis], periods[axis]) - next.start[axis]
                });
                next.shift(shift);
            }
            gaps.push((end, next.start, degenerate(map, end, next.start)));
        }
        traces.push(next);
    }
    let (first, last) = (traces.first()?.start, traces.last()?.end);
    if degenerate(map, last, first) {
        gaps.push((last, first, true));
    } else {
        let close: [f64; 2] = std::array::from_fn(|axis| match periods[axis] {
            Some(period) => first[axis] + ((last[axis] - first[axis]) / period).round() * period,
            None => first[axis],
        });
        gaps.push((last, close, false));
    }
    // How far the loop runs round each wrapped parameter. A loop crossing a
    // pole goes along it by whatever its trims' turns left over, and a loop
    // that does is no band: the pole takes up the winding.
    let travel = |axis: usize, gaps: &[([f64; 2], [f64; 2], bool)]| {
        traces.iter().map(|trace| trace.end[axis] - trace.start[axis]).sum::<f64>()
            + gaps.iter().map(|(from, to, _)| to[axis] - from[axis]).sum::<f64>()
    };
    if let (Some(period), Some(pole)) = (periods[0], gaps.iter().position(|gap| gap.2)) {
        let turns = (travel(0, &gaps) / period).round();
        gaps[pole].1[0] -= turns * period;
    }
    let winding = std::array::from_fn(|axis| match periods[axis] {
        Some(period) => (travel(axis, &gaps) / period).round() as i64,
        None => 0,
    });
    Some(Ring { traces, gaps, winding })
}

/// A loop running along two poles or more — a whole sphere's seam, up and
/// back — goes round the same way whichever way the poles are crossed, so
/// its turn there is the face's: counter-clockwise in `(a, b)` when `left`.
fn orient_poles(ring: &mut Ring<'_>, left: bool) {
    if ring.gaps.iter().filter(|gap| gap.2).count() < 2 {
        return;
    }
    // −∮ b da, by the samples.
    let mut area = 0.0;
    for trace in &ring.traces {
        for pair in trace.samples.windows(2) {
            area -= 0.5 * (pair[0][1] + pair[1][1]) * (pair[1][0] - pair[0][0]);
        }
    }
    for (from, to, _) in &ring.gaps {
        area -= 0.5 * (from[1] + to[1]) * (to[0] - from[0]);
    }
    if (area > 0.0) != left {
        for (from, to, pole) in &mut ring.gaps {
            if *pole {
                to[0] = 2.0 * from[0] - to[0];
            }
        }
    }
}

/// The level of `b` at a pole of the surface on the side the face lies:
/// above its loops when `above`, else below.
fn pole_level(map: &FaceMap<'_>, above: bool) -> Option<f64> {
    if map.swap {
        return None;
    }
    match map.surface {
        Surface::Sphere(_) => Some(if above { PI / 2.0 } else { -PI / 2.0 }),
        Surface::Cone(cone) if cone.half_angle.tan() != 0.0 => Some(cone.radius / cone.half_angle.tan()),
        _ => None,
    }
}

/// The face's [`Moments`], or `None` where its trims cannot be followed.
fn face_moments(body: &Body, face_key: super::topology::FaceKey, centre: Vec3, scale: f64) -> Option<Moments> {
    let face = body.faces.get(face_key)?;
    let surface = body.surfaces.get(face.surface)?;
    if face.loops.is_empty() {
        return whole_surface_moments(surface, face.forward, centre, scale);
    }
    // Where each trim starts in the surface's parameters, placed turn by turn
    // so the loop joins up, which decides which side of a seam each use of it
    // is on.
    let periodic = super::pcurve::periods(surface) != [None, None];
    let anchors: Vec<Option<[f64; 2]>> = if periodic {
        let parts = super::pcurve::face_boundary_parts(body, face_key, 1e-7 * scale)?;
        parts.iter().map(|(_, curve)| Some(curve.point_at(0.0))).collect()
    } else {
        Vec::new()
    };
    for swap in [false, true] {
        let map = FaceMap { surface, swap, centre, scale };
        let mut rings = Vec::with_capacity(face.loops.len());
        let mut offset = 0;
        for loop_key in &face.loops {
            let coedges = &body.loops.get(*loop_key)?.coedges;
            let anchors: Vec<Option<[f64; 2]>> = anchors
                .get(offset..offset + coedges.len())
                .unwrap_or(&[])
                .iter()
                .map(|anchor| anchor.map(|uv| map.ab((uv[0], uv[1]))))
                .collect();
            offset += coedges.len();
            rings.push(ring(body, &map, coedges, &anchors)?);
        }
        // A loop wrapping round `b` leaves the face's iso-`b` integrals
        // without the seam that would bound them: integrate the other way.
        if rings.iter().any(|ring| ring.winding[1] != 0) {
            if rings.iter().any(|ring| ring.winding[0] != 0) {
                return None;
            }
            continue;
        }
        // Loops winding round `a` with nothing to wind back leave a pole
        // inside the face, a line in its parameters no edge runs along.
        let unwound: i64 = rings.iter().map(|ring| ring.winding[0]).sum();
        let mut pole = None;
        if unwound != 0 {
            let period = map.periods()[0]?;
            let above = (unwound > 0) == (face.forward != swap);
            let level = pole_level(&map, above)?;
            pole = Some(([0.0, level], [-(unwound as f64) * period, level]));
        }
        place_bands(&map, face.forward, &mut rings);
        if let Some(outer) = rings.first_mut() {
            orient_poles(outer, face.forward != swap);
        }
        let base = rings
            .iter()
            .flat_map(|ring| ring.traces.iter())
            .flat_map(|trace| trace.samples.iter())
            .map(|sample| sample[1])
            .fold(f64::INFINITY, f64::min);
        if !base.is_finite() {
            return None;
        }
        let knots = map.b_knots();
        let mut total = [0.0; 10];
        for ring in &rings {
            for trace in &ring.traces {
                let part = trace_integral(&map, trace, base, &knots)?;
                total.iter_mut().zip(part).for_each(|(sum, value)| *sum += value);
            }
            for (from, to, _) in &ring.gaps {
                let part = gap_integral(&map, *from, *to, base, &knots)?;
                total.iter_mut().zip(part).for_each(|(sum, value)| *sum += value);
            }
        }
        if let Some((from, to)) = pole {
            let part = gap_integral(&map, from, to, base, &knots)?;
            total.iter_mut().zip(part).for_each(|(sum, value)| *sum += value);
        }
        return Some(total);
    }
    None
}

/// On a surface wrapped in `b` too, puts every loop that winds round `a` on
/// the turn of `b` that keeps the face between them: a loop wound one way
/// has the face above it, wound the other way below it.
fn place_bands(map: &FaceMap<'_>, forward: bool, rings: &mut [Ring<'_>]) {
    let Some(period) = map.periods()[1] else {
        return;
    };
    let level = |ring: &Ring<'_>| ring.traces.first().map_or(0.0, |trace| trace.start[1]);
    let Some(reference) = rings.iter().find(|ring| ring.winding[0] != 0) else {
        return;
    };
    let (reference_level, reference_winding) = (level(reference), reference.winding[0]);
    // Counter-clockwise in `(a, b)` exactly when the face runs with its
    // surface and the parameters are in their own order.
    let left = forward != map.swap;
    let above = (reference_winding > 0) == left;
    for ring in rings.iter_mut().filter(|ring| ring.winding[0] != 0) {
        let here = level(ring);
        if here == reference_level {
            continue;
        }
        let mut turns = ((here - reference_level) / period).floor();
        if !above {
            turns += 1.0;
        }
        let shift = [0.0, -turns * period];
        for trace in &mut ring.traces {
            trace.shift(shift);
        }
        for gap in &mut ring.gaps {
            gap.0[1] += shift[1];
            gap.1[1] += shift[1];
        }
    }
}

/// A face with no loops: the whole of a closed surface.
fn whole_surface_moments(surface: &Surface, forward: bool, centre: Vec3, scale: f64) -> Option<Moments> {
    use std::f64::consts::FRAC_PI_2;
    let ((a0, a1), (b0, b1)) = match surface {
        Surface::Sphere(_) => ((-PI, PI), (-FRAC_PI_2, FRAC_PI_2)),
        Surface::Torus(_) => ((-PI, PI), (-PI, PI)),
        Surface::Nurbs(_) | Surface::Offset(_) => surface.spline()?.domain(),
        _ => return None,
    };
    let map = FaceMap { surface, swap: false, centre, scale };
    let knots = map.b_knots();
    let a_knots = match surface.spline() {
        Some(spline) => spline.knots().0.to_vec(),
        None => Vec::new(),
    };
    let total = kronrod_split(a0, a1, &a_knots, ACCURACY, 1.0, |a| {
        map.iso_integral([a, b1], b0, &knots, 1.0 / (a1 - a0))
    })?;
    let sign = if forward { 1.0 } else { -1.0 };
    Some(total.map(|value| value * sign))
}

/// Mass properties integrated over the body's faces, each bounded by its
/// trims traced along the edges' own curves: exact to the quadrature's
/// tolerance, about ten digits. `None` for a body that is not closed, or
/// whose trims cannot all be followed on their surfaces.
pub fn integrated_mass_properties(body: &Body) -> Option<MassProperties> {
    if body.edges.iter().any(|(_, edge)| edge.coedges.len() != 2) || body.faces.iter().next().is_none() {
        return None;
    }
    let bounds = super::bounds::body_bounds(body)?;
    let (low, high) = (Vec3::from(bounds.min), Vec3::from(bounds.max));
    let centre = (low + high) * 0.5;
    let scale = (high - low).length() * 0.5;
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    let mut total = [0.0; 10];
    for face in body.face_keys() {
        let part = face_moments(body, face, centre, scale)?;
        total.iter_mut().zip(part).for_each(|(sum, value)| *sum += value);
    }
    from_moments(total, centre, scale)
}

/// Mass properties from [`Moments`] taken about `centre` in units of
/// `scale`.
fn from_moments(moments: Moments, centre: Vec3, scale: f64) -> Option<MassProperties> {
    let [v, x, y, z, xx, yy, zz, xy, yz, zx] = moments;
    let volume = v * scale.powi(3);
    if !(volume.is_finite() && volume > 0.0) {
        return None;
    }
    let first = [x, y, z].map(|value| value * scale.powi(4));
    let [xx, yy, zz, xy, yz, zx] = [xx, yy, zz, xy, yz, zx].map(|value| value * scale.powi(5));
    let local = first.map(|value| value / volume);
    let [gx, gy, gz] = local;
    let central = [
        [yy + zz - volume * (gy * gy + gz * gz), -(xy - volume * gx * gy), -(zx - volume * gz * gx)],
        [-(xy - volume * gx * gy), xx + zz - volume * (gx * gx + gz * gz), -(yz - volume * gy * gz)],
        [-(zx - volume * gz * gx), -(yz - volume * gy * gz), xx + yy - volume * (gx * gx + gy * gy)],
    ];
    let (principal_moments, principal_directions) = super::mesh::principal_axes(central);
    let centroid = (centre + Vec3::from(local)).to_array();
    let c2 = centroid.iter().map(|value| value * value).sum::<f64>();
    let origin: [[f64; 3]; 3] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            central[row][column]
                + volume * (if row == column { c2 } else { 0.0 } - centroid[row] * centroid[column])
        })
    });
    let moment_of_inertia = [origin[0][0], origin[1][1], origin[2][2]];
    let properties = MassProperties {
        volume,
        centroid,
        moment_of_inertia,
        principal_directions,
        principal_moments,
        product_of_inertia: [origin[0][1], origin[1][2], origin[2][0]],
        radii_of_gyration: moment_of_inertia.map(|moment| (moment.max(0.0) / volume).sqrt()),
    };
    centroid
        .iter()
        .chain(&properties.moment_of_inertia)
        .chain(&properties.principal_moments)
        .chain(&properties.product_of_inertia)
        .all(|value| value.is_finite())
        .then_some(properties)
}

fn cylindrical_sector_properties(body: &Body) -> Option<MassProperties> {
    let mut cylinders = Vec::new();
    for face_key in body.face_keys() {
        let face = body.faces.get(face_key)?;
        match body.surfaces.get(face.surface)? {
            Surface::Cylinder(cylinder) => cylinders.push((face_key, *cylinder)),
            Surface::Plane(_) => {}
            _ => return None,
        }
    }
    if cylinders.len() != 2 {
        return None;
    }
    cylinders.sort_by(|left, right| right.1.radius.total_cmp(&left.1.radius));
    let (outer_face, outer) = cylinders[0];
    let (inner_face, inner) = cylinders[1];
    let axis = Vec3::from(outer.base.normal()?).normalize()?;
    let inner_axis = Vec3::from(inner.base.normal()?).normalize()?;
    let scale = outer.radius.abs().max(1.0);
    let outer_origin = Vec3::from(outer.base.origin);
    let inner_origin = Vec3::from(inner.base.origin);
    let inner_lateral = inner_origin - axis * (inner_origin - outer_origin).dot(axis);
    if !outer.radius.is_finite()
        || !inner.radius.is_finite()
        || inner.radius <= 0.0
        || inner.radius >= outer.radius
        || inner_axis.dot(axis).abs() < 1.0 - 1e-8
        || inner_lateral.distance(outer_origin) > scale * 1e-8
    {
        return None;
    }
    let patch = super::thicken::rectangular_patch(body, outer_face, &Surface::Cylinder(outer))?;
    let inner_patch =
        super::thicken::rectangular_patch(body, inner_face, &Surface::Cylinder(inner))?;
    let inner_bottom = inner_origin + inner_axis * inner_patch.v_start;
    let inner_top = inner_bottom + inner_axis * inner_patch.v_sweep;
    let outer_bottom = patch.v_start;
    let outer_top = patch.v_start + patch.v_sweep;
    let z0 = (inner_bottom - outer_origin).dot(axis);
    let z1 = (inner_top - outer_origin).dot(axis);
    if (z0.min(z1) - outer_bottom).abs() > scale * 1e-8
        || (z0.max(z1) - outer_top).abs() > scale * 1e-8
        || (inner_patch.u_sweep - patch.u_sweep).abs() > 1e-8
        || body.edges.iter().any(|(_, edge)| edge.coedges.len() != 2)
    {
        return None;
    }
    let outer_mid = Vec3::from(outer.base.vector_at([
        (patch.u_start + patch.u_sweep * 0.5).cos(),
        (patch.u_start + patch.u_sweep * 0.5).sin(),
    ]))
    .normalize()?;
    let inner_mid = Vec3::from(inner.base.vector_at([
        (inner_patch.u_start + inner_patch.u_sweep * 0.5).cos(),
        (inner_patch.u_start + inner_patch.u_sweep * 0.5).sin(),
    ]))
    .normalize()?;
    if patch.u_sweep < 2.0 * PI - 1e-8 && outer_mid.distance(inner_mid) > 1e-8 {
        return None;
    }
    let angle = patch.u_sweep;
    let height = patch.v_sweep;
    if !angle.is_finite()
        || !height.is_finite()
        || angle <= 0.0
        || angle > 2.0 * PI + 1e-8
        || height <= 0.0
    {
        return None;
    }

    let radial_square = outer.radius.powi(2) - inner.radius.powi(2);
    let radial_cube = outer.radius.powi(3) - inner.radius.powi(3);
    let radial_fourth = outer.radius.powi(4) - inner.radius.powi(4);
    let area = 0.5 * radial_square * angle;
    let half = angle * 0.5;
    let centroid_radius = if (angle - 2.0 * PI).abs() <= 1e-8 {
        0.0
    } else {
        4.0 * half.sin() * radial_cube / (3.0 * angle * radial_square)
    };
    let volume = area * height;
    let middle = patch.u_start + half;
    let radial = Vec3::from(outer.base.vector_at([middle.cos(), middle.sin()])).normalize()?;
    let tangent = axis.cross(radial).normalize()?;
    let centre_on_axis = outer_origin + axis * (patch.v_start + height * 0.5);
    let centroid = (centre_on_axis + radial * centroid_radius).to_array();

    let factor = radial_fourth * 0.25;
    let radial_second = factor * (angle * 0.5 + angle.sin() * 0.5);
    let tangent_second = factor * (angle * 0.5 - angle.sin() * 0.5);
    let height_second = area * height.powi(3) / 12.0;
    let radial_moment = height * tangent_second + height_second;
    let tangent_moment = height * (radial_second - area * centroid_radius.powi(2)) + height_second;
    let axial_moment = height * (radial_second + tangent_second - area * centroid_radius.powi(2));
    assemble(
        volume,
        centroid,
        [radial_moment, tangent_moment, axial_moment],
        [radial.to_array(), tangent.to_array(), axis.to_array()],
    )
}

fn sphere_properties(body: &Body) -> Option<MassProperties> {
    let mut spheres = Vec::new();
    for face_key in body.face_keys() {
        let face = body.faces.get(face_key)?;
        let Surface::Sphere(sphere) = body.surfaces.get(face.surface)? else {
            return None;
        };
        spheres.push(*sphere);
    }
    if !(1..=2).contains(&spheres.len()) {
        return None;
    }
    spheres.sort_by(|left, right| right.radius.total_cmp(&left.radius));
    let center = Vec3::from(spheres[0].frame.origin);
    let scale = spheres[0].radius.abs().max(1.0);
    if spheres.iter().any(|sphere| {
        !sphere.radius.is_finite()
            || sphere.radius <= 0.0
            || center.distance(Vec3::from(sphere.frame.origin)) > scale * 1e-8
    }) {
        return None;
    }
    let outer = spheres[0].radius;
    let inner = spheres.get(1).map_or(0.0, |sphere| sphere.radius);
    if inner >= outer {
        return None;
    }
    let volume = 4.0 * PI * (outer.powi(3) - inner.powi(3)) / 3.0;
    let principal = 8.0 * PI * (outer.powi(5) - inner.powi(5)) / 15.0;
    assemble(
        volume,
        center.to_array(),
        [principal; 3],
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    )
}

fn cylinder_properties(body: &Body) -> Option<MassProperties> {
    let mut cylinders = Vec::new();
    let mut planes = Vec::new();
    for face_key in body.face_keys() {
        let face = body.faces.get(face_key)?;
        match body.surfaces.get(face.surface)? {
            Surface::Cylinder(cylinder) => cylinders.push(*cylinder),
            Surface::Plane(plane) => planes.push(*plane),
            _ => return None,
        }
    }
    if cylinders.is_empty() || planes.is_empty() {
        return None;
    }
    cylinders.sort_by(|left, right| right.radius.total_cmp(&left.radius));
    let outer = cylinders[0];
    let axis = Vec3::from(outer.base.normal()?).normalize()?;
    let x = Vec3::from(outer.base.x_axis).normalize()?;
    let x = (x - axis * x.dot(axis)).normalize()?;
    let y = axis.cross(x).normalize()?;
    let reference = Vec3::from(outer.base.origin);
    let reference_lateral = reference - axis * reference.dot(axis);
    let scale = outer.radius.abs().max(1.0);
    if cylinders.iter().any(|cylinder| {
        if !cylinder.radius.is_finite() || cylinder.radius <= 0.0 {
            return true;
        }
        let Some(candidate_normal) = cylinder.base.normal() else {
            return true;
        };
        let Some(candidate_axis) = Vec3::from(candidate_normal).normalize() else {
            return true;
        };
        let origin = Vec3::from(cylinder.base.origin);
        let lateral = origin - axis * origin.dot(axis);
        candidate_axis.dot(axis).abs() < 1.0 - 1e-8
            || lateral.distance(reference_lateral) > scale * 1e-8
    }) {
        return None;
    }
    cylinders.dedup_by(|left, right| (left.radius - right.radius).abs() <= scale * 1e-8);
    if !(1..=2).contains(&cylinders.len()) {
        return None;
    }
    let mut positions = planes
        .iter()
        .map(|plane| {
            let normal = Vec3::from(plane.normal()?);
            (normal.dot(axis).abs() >= 1.0 - 1e-8)
                .then(|| (Vec3::from(plane.origin) - reference).dot(axis))
        })
        .collect::<Option<Vec<_>>>()?;
    positions.sort_by(f64::total_cmp);
    let tolerance = positions
        .iter()
        .map(|value| value.abs())
        .fold(scale, f64::max)
        * 1e-8;
    positions.dedup_by(|left, right| (*left - *right).abs() <= tolerance);
    if positions.len() != cylinders.len() * 2 {
        return None;
    }

    let outer_min = positions[0];
    let outer_max = *positions.last()?;
    let outer_height = outer_max - outer_min;
    if outer_height <= 0.0 {
        return None;
    }
    let outer_volume = PI * outer.radius.powi(2) * outer_height;
    let outer_center = (outer_min + outer_max) * 0.5;

    let (inner_radius, inner_height, inner_volume, inner_center) = if cylinders.len() == 2 {
        let inner = cylinders[1];
        if inner.radius >= outer.radius {
            return None;
        }
        let inner_min = positions[1];
        let inner_max = positions[2];
        let height = inner_max - inner_min;
        if height <= 0.0 {
            return None;
        }
        (
            inner.radius,
            height,
            PI * inner.radius.powi(2) * height,
            (inner_min + inner_max) * 0.5,
        )
    } else {
        (0.0, 0.0, 0.0, outer_center)
    };
    let volume = outer_volume - inner_volume;
    if !volume.is_finite() || volume <= 0.0 {
        return None;
    }
    let center_at = (outer_volume * outer_center - inner_volume * inner_center) / volume;
    let centroid = (reference + axis * center_at).to_array();
    let axial = 0.5 * (outer_volume * outer.radius.powi(2) - inner_volume * inner_radius.powi(2));
    let transverse = outer_volume * (3.0 * outer.radius.powi(2) + outer_height.powi(2)) / 12.0
        + outer_volume * (outer_center - center_at).powi(2)
        - inner_volume * (3.0 * inner_radius.powi(2) + inner_height.powi(2)) / 12.0
        - inner_volume * (inner_center - center_at).powi(2);
    assemble(
        volume,
        centroid,
        [transverse, transverse, axial],
        [x.to_array(), y.to_array(), axis.to_array()],
    )
}

fn assemble(
    volume: f64,
    centroid: [f64; 3],
    principal_moments: [f64; 3],
    axes: [[f64; 3]; 3],
) -> Option<MassProperties> {
    if !volume.is_finite()
        || volume <= 0.0
        || centroid
            .iter()
            .chain(principal_moments.iter())
            .any(|value| !value.is_finite())
    {
        return None;
    }
    let mut central = [[0.0; 3]; 3];
    for principal in 0..3 {
        for row in 0..3 {
            for column in 0..3 {
                central[row][column] +=
                    principal_moments[principal] * axes[principal][row] * axes[principal][column];
            }
        }
    }
    let c = Vec3::from(centroid);
    let c2 = c.dot(c);
    let mut origin = central;
    for row in 0..3 {
        for column in 0..3 {
            origin[row][column] +=
                volume * (if row == column { c2 } else { 0.0 } - centroid[row] * centroid[column]);
        }
    }
    let moment_of_inertia = [origin[0][0], origin[1][1], origin[2][2]];
    let product_of_inertia = [origin[0][1], origin[1][2], origin[2][0]];
    let radii_of_gyration = moment_of_inertia.map(|moment| (moment.max(0.0) / volume).sqrt());
    Some(MassProperties {
        volume,
        centroid,
        moment_of_inertia,
        principal_directions: [
            axes[0][0], axes[0][1], axes[0][2], axes[1][0], axes[1][1], axes[1][2], axes[2][0],
            axes[2][1], axes[2][2],
        ],
        principal_moments,
        product_of_inertia,
        radii_of_gyration,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make;

    #[test]
    fn a_translated_sphere_has_exact_mass_properties() {
        let body = make::sphere([2.0, -3.0, 5.0], 2.0).unwrap();
        let properties = analytic_mass_properties(&body).unwrap();

        assert!((properties.volume - 32.0 * PI / 3.0).abs() < 1e-10);
        assert_eq!(properties.centroid, [2.0, -3.0, 5.0]);
        let central = 2.0 * properties.volume * 4.0 / 5.0;
        assert!(properties
            .principal_moments
            .iter()
            .all(|moment| (*moment - central).abs() < 1e-10));
    }

    #[test]
    fn a_cylinder_has_exact_volume_centroid_and_principal_moments() {
        let body = make::cylinder([1.0, 2.0, 3.0], 2.0, 6.0).unwrap();
        let properties = analytic_mass_properties(&body).unwrap();

        let volume = 24.0 * PI;
        assert!((properties.volume - volume).abs() < 1e-10);
        assert_eq!(properties.centroid, [1.0, 2.0, 6.0]);
        assert!((properties.principal_moments[0] - 4.0 * volume).abs() < 1e-10);
        assert!((properties.principal_moments[1] - 4.0 * volume).abs() < 1e-10);
        assert!((properties.principal_moments[2] - 2.0 * volume).abs() < 1e-10);
    }

    #[test]
    fn translating_properties_matches_translating_the_body() {
        let properties = analytic_mass_properties(&make::sphere([2.0, -3.0, 5.0], 2.0).unwrap())
            .unwrap()
            .translated([7.0, 11.0, -13.0]);
        let expected =
            analytic_mass_properties(&make::sphere([9.0, 8.0, -8.0], 2.0).unwrap()).unwrap();

        assert_eq!(properties.centroid, expected.centroid);
        for (actual, expected) in properties
            .moment_of_inertia
            .into_iter()
            .chain(properties.product_of_inertia)
            .zip(
                expected
                    .moment_of_inertia
                    .into_iter()
                    .chain(expected.product_of_inertia),
            )
        {
            assert!((actual - expected).abs() < 1e-9);
        }
    }
}

#[cfg(test)]
mod integrated_tests {
    use super::*;
    use crate::make;

    fn close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance * expected.abs().max(1.0),
            "{actual} != {expected}"
        );
    }

    #[test]
    fn a_box_integrates_to_its_exact_properties() {
        let body = make::cuboid([1.0, 2.0, 3.0], [2.0, 3.0, 4.0]).unwrap();
        let properties = integrated_mass_properties(&body).unwrap();
        close(properties.volume, 24.0, 1e-12);
        for (actual, expected) in properties.centroid.into_iter().zip([2.0, 3.5, 5.0]) {
            close(actual, expected, 1e-12);
        }
        let mut moments = properties.principal_moments;
        moments.sort_by(f64::total_cmp);
        let mut expected = [9.0 + 16.0, 4.0 + 16.0, 4.0 + 9.0].map(|sum| 24.0 * sum / 12.0);
        expected.sort_by(f64::total_cmp);
        for (actual, expected) in moments.into_iter().zip(expected) {
            close(actual, expected, 1e-11);
        }
    }

    #[test]
    fn curved_primitives_integrate_to_their_closed_forms() {
        let sphere = integrated_mass_properties(&make::sphere([1.0, -2.0, 0.5], 2.0).unwrap()).unwrap();
        close(sphere.volume, 32.0 * PI / 3.0, 1e-10);
        for moment in sphere.principal_moments {
            close(moment, 2.0 * sphere.volume * 4.0 / 5.0, 1e-9);
        }
        for (actual, expected) in sphere.centroid.into_iter().zip([1.0, -2.0, 0.5]) {
            close(actual, expected, 1e-10);
        }

        let cylinder = integrated_mass_properties(&make::cylinder([0.0, 0.0, 1.0], 2.0, 6.0).unwrap()).unwrap();
        close(cylinder.volume, 24.0 * PI, 1e-10);
        close(cylinder.centroid[2], 4.0, 1e-10);

        // Apex and rim: a quarter of the height up, a third of the cone.
        let cone = integrated_mass_properties(&make::cone([0.0, 0.0, 0.0], 3.0, 4.0).unwrap()).unwrap();
        close(cone.volume, 12.0 * PI, 1e-10);
        close(cone.centroid[2], 1.0, 1e-10);
        let axial = 0.3 * cone.volume * 9.0;
        assert!(cone.principal_moments.iter().any(|moment| (moment - axial).abs() < 1e-8 * axial));

        let torus = integrated_mass_properties(&make::torus([0.0, 0.0, 0.0], 3.0, 1.0).unwrap()).unwrap();
        close(torus.volume, 2.0 * PI * PI * 3.0, 1e-10);
        let axial = torus.volume * (9.0 + 0.75);
        assert!(torus.principal_moments.iter().any(|moment| (moment - axial).abs() < 1e-8 * axial));
    }

    #[test]
    fn a_bored_block_integrates_exactly_where_the_mesh_only_converges() {
        let block = make::cuboid([0.0, 0.0, 0.0], [4.0, 4.0, 2.0]).unwrap();
        let bore = make::cylinder([2.0, 2.0, -1.0], 1.0, 4.0).unwrap();
        let body = crate::combine(block, bore, crate::Operation::Difference, 1e-9).unwrap();
        let properties = integrated_mass_properties(&body).unwrap();
        close(properties.volume, 32.0 - 2.0 * PI, 1e-10);
        for (actual, expected) in properties.centroid.into_iter().zip([2.0, 2.0, 1.0]) {
            close(actual, expected, 1e-10);
        }
        let meshed = meshed_mass_properties(&body).unwrap();
        close(meshed.volume, properties.volume, 1e-5);
    }

    #[test]
    fn spline_walls_integrate_to_their_closed_forms() {
        use ocs_geom::geom2d::{Curve as Curve2, Line, NurbsCurve};
        use ocs_geom::space::Plane;
        let arch = |points: Vec<[f64; 2]>, back: Line| {
            vec![Curve2::Nurbs(NurbsCurve::new(2, points, Vec::new(), None).unwrap()), Curve2::Line(back)]
        };
        // A parabolic segment two thirds of its box, its centroid two fifths
        // of its height up.
        let profile = arch(vec![[0.0, 0.0], [5.0, 5.0], [10.0, 0.0]], Line { start: [10.0, 0.0], end: [0.0, 0.0] });
        let prism = crate::sweep::extrude(Plane::XY, &profile, [0.0, 0.0, 1.0]).unwrap();
        let properties = integrated_mass_properties(&prism).unwrap();
        close(properties.volume, 50.0 / 3.0, 1e-10);
        for (actual, expected) in properties.centroid.into_iter().zip([5.0, 1.0, 0.5]) {
            close(actual, expected, 1e-10);
        }

        // x = 1 + 6t(1 - t), z = 4t turned about z: 4π ∫ 12s + 36s² dt.
        let half = Plane::from_axes([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let profile = arch(vec![[1.0, 0.0], [4.0, 2.0], [1.0, 4.0]], Line { start: [1.0, 4.0], end: [1.0, 0.0] });
        let turned = crate::sweep::revolve(half, &profile, [0.0; 3], [0.0, 0.0, 1.0], std::f64::consts::TAU).unwrap();
        let properties = integrated_mass_properties(&turned).unwrap();
        close(properties.volume, 12.8 * PI, 1e-10);
        close(properties.centroid[2], 2.0, 1e-10);
    }

    #[test]
    fn a_sphere_cut_by_a_plane_integrates_across_its_pole() {
        let ball = make::sphere([0.0; 3], 2.0).unwrap();
        let lid = make::cuboid([-3.0, -3.0, 1.0], [6.0, 6.0, 3.0]).unwrap();
        let body = crate::combine(ball, lid, crate::Operation::Difference, 1e-9).unwrap();
        // Less a cap one high: π h² (3R − h) / 3.
        let properties = integrated_mass_properties(&body).unwrap();
        close(properties.volume, 32.0 * PI / 3.0 - 5.0 * PI / 3.0, 1e-10);
        let flipped = crate::combine(
            make::cuboid([-3.0, -3.0, -3.0], [6.0, 6.0, 6.0]).unwrap(),
            make::sphere([0.0; 3], 2.0).unwrap(),
            crate::Operation::Difference,
            1e-9,
        )
        .unwrap();
        close(integrated_mass_properties(&flipped).unwrap().volume, 216.0 - 32.0 * PI / 3.0, 1e-10);
    }

    #[test]
    fn a_filleted_rim_integrates_its_torus_band() {
        let (radius, height, round) = (3.0, 4.0, 1.0);
        let cylinder = make::cylinder([0.0; 3], radius, height).unwrap();
        let rim = cylinder
            .edge_keys()
            .find(|edge| {
                let edge = cylinder.edges.get(*edge).unwrap();
                matches!(cylinder.curves.get(edge.curve), Some(Curve3::Circle(_)))
                    && cylinder.vertices.get(edge.start).unwrap().point[2] > height - 1e-9
            })
            .unwrap();
        let body = crate::fillet(&cylinder, rim, round).unwrap();
        // Pappus: the corner a quarter disc leaves, turned round the axis.
        let corner = round * round * (1.0 - PI / 4.0);
        let reach = radius - round + round / (6.0 * (1.0 - PI / 4.0));
        let expected = PI * radius * radius * height - 2.0 * PI * reach * corner;
        let properties = integrated_mass_properties(&body).unwrap();
        close(properties.volume, expected, 1e-10);
    }
}

