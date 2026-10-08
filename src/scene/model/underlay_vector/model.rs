//! Vector content of a DWF sheet or DGN model, in the underlay's own units
//! (the sheet's model units), and what an underlay needs from it.

/// One segment of a path.
#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Line([f64; 2], [f64; 2]),
    Cubic([f64; 2], [f64; 2], [f64; 2], [f64; 2]),
}

impl Segment {
    pub fn start(&self) -> [f64; 2] {
        match self {
            Segment::Line(a, _) | Segment::Cubic(a, ..) => *a,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubPath {
    pub segments: Vec<Segment>,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub subpaths: Vec<SubPath>,
    /// Stroke colour and width (sheet units; 0 draws one pixel wide).
    pub stroke: Option<([u8; 3], f64)>,
    pub fill: Option<[u8; 3]>,
}

/// A sheet (DWF) or model (DGN): its extent and what it draws.
#[derive(Clone, Debug, Default)]
pub struct Sheet {
    /// min x, min y, max x, max y in sheet units.
    pub rect: [f64; 4],
    pub paths: Vec<Path>,
    pub texts: Vec<Text>,
    /// DGN: sub units per master unit (the scale a Sub conversion offers).
    pub sub_per_master: f64,
    /// DGN: the colours come from the file's own colour table, which the
    /// reference shows adapted to the background ([`dgn_table_color`]).
    pub table_colors: bool,
    /// The colour of each path and text the sheet draws, one per element.
    pub element_colors: Vec<[u8; 3]>,
}

/// How a colour from a DGN file's colour table shows over a background
/// (its largest channel and whether it is light), as the reference draws
/// it: a near-white colour (61 or less from white, channels summed) is white,
/// or black on a light background; over a background with a channel of 32 or
/// more, colours whose largest channel is 76, 127, 153 or 204 drop a shade
/// (to 38, 76, 127 or 165, hue kept) and the greys 91, 132, 173 and 214 drop
/// to 45, 91, 137 and 183 — grey 51 to black from a channel of 30.
pub fn dgn_table_color(rgb: [u8; 3], bg_max: u8, light: bool) -> [u8; 3] {
    if rgb.iter().map(|&c| 255 - c as u32).sum::<u32>() <= 61 {
        return if light { [0; 3] } else { [255; 3] };
    }
    if rgb[0] == rgb[1] && rgb[1] == rgb[2] {
        let v = match rgb[0] {
            51 if bg_max >= 30 => 0,
            91 if bg_max >= 32 => 45,
            132 if bg_max >= 32 => 91,
            173 if bg_max >= 32 => 137,
            214 if bg_max >= 32 => 183,
            v => v,
        };
        return [v; 3];
    }
    let top = rgb.into_iter().max().unwrap_or(0);
    let to = match top {
        76 => 38,
        127 => 76,
        153 => 127,
        204 => 165,
        _ => return rgb,
    };
    if bg_max < 32 {
        return rgb;
    }
    rgb.map(|c| ((c as u32 * to + top as u32 / 2) / top as u32) as u8)
}

/// Builds paths from pen moves.
#[derive(Default)]
pub struct PathBuilder {
    pub subpaths: Vec<SubPath>,
    current: Vec<Segment>,
    start: Option<[f64; 2]>,
    at: Option<[f64; 2]>,
}

impl PathBuilder {
    pub fn move_to(&mut self, p: [f64; 2]) {
        self.flush(false);
        self.start = Some(p);
        self.at = Some(p);
    }

    pub fn line_to(&mut self, p: [f64; 2]) {
        let Some(a) = self.at else {
            self.move_to(p);
            return;
        };
        self.current.push(Segment::Line(a, p));
        self.at = Some(p);
    }

    pub fn cubic_to(&mut self, c1: [f64; 2], c2: [f64; 2], p: [f64; 2]) {
        let Some(a) = self.at else {
            self.move_to(p);
            return;
        };
        self.current.push(Segment::Cubic(a, c1, c2, p));
        self.at = Some(p);
    }

    pub fn close(&mut self) {
        if let (Some(s), Some(a)) = (self.start, self.at) {
            if s != a {
                self.current.push(Segment::Line(a, s));
            }
        }
        self.flush(true);
        self.at = self.start;
    }

    pub fn current(&self) -> Option<[f64; 2]> {
        self.at
    }

    fn flush(&mut self, closed: bool) {
        if !self.current.is_empty() {
            self.subpaths.push(SubPath { segments: std::mem::take(&mut self.current), closed });
        }
    }

    pub fn finish(mut self) -> Vec<SubPath> {
        self.flush(false);
        self.subpaths
    }
}

/// Cubic pieces of an elliptical arc: centre, radii, axis rotation and the
/// start and sweep angles (radians, counter-clockwise positive).
pub fn arc_cubics(
    center: [f64; 2],
    rx: f64,
    ry: f64,
    rotation: f64,
    start: f64,
    sweep: f64,
) -> Vec<[[f64; 2]; 4]> {
    // The sweep can be a raw f64 out of file bytes (DGN `f64_at(e, 112)`):
    // 1e300 or inf saturates the `as usize` cast to `usize::MAX` and
    // `Vec::with_capacity` panics `capacity overflow`; ~1e17 would ask
    // for 2 EB and abort. Legitimate callers top out near 104 rad (DGN
    // V7, ±5965°), so the sweep is clamped at 4096 pieces' worth; NaN
    // keeps its old single-degenerate-piece behaviour.
    const MAX_SWEEP_MAGNITUDE: f64 = 4096.0 * std::f64::consts::FRAC_PI_2;
    let sweep = if sweep.is_nan() || sweep.abs() <= MAX_SWEEP_MAGNITUDE {
        sweep
    } else {
        sweep.signum() * MAX_SWEEP_MAGNITUDE
    };
    let n = ((sweep.abs() / (std::f64::consts::FRAC_PI_2)).ceil() as usize).max(1);
    let step = sweep / n as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let (cr, sr) = (rotation.cos(), rotation.sin());
    let at = |x: f64, y: f64| [center[0] + x * cr - y * sr, center[1] + x * sr + y * cr];
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a0 = start + step * i as f64;
        let a1 = a0 + step;
        let (c0, s0) = (a0.cos(), a0.sin());
        let (c1, s1) = (a1.cos(), a1.sin());
        let p0 = at(rx * c0, ry * s0);
        let p3 = at(rx * c1, ry * s1);
        let p1 = at(rx * (c0 - k * s0), ry * (s0 + k * c0));
        let p2 = at(rx * (c1 + k * s1), ry * (s1 - k * c1));
        out.push([p0, p1, p2, p3]);
    }
    out
}

/// Tight bounds of the paths (curves by their extreme points, not their
/// control points), when there are any.
pub fn paths_bounds(paths: &[Path]) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut add = |p: [f64; 2]| {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    };
    for path in paths {
        for sp in &path.subpaths {
            for seg in &sp.segments {
                match seg {
                    Segment::Line(a, c) => {
                        add(*a);
                        add(*c);
                    }
                    Segment::Cubic(a, c1, c2, c) => {
                        add(*a);
                        add(*c);
                        for t in cubic_extrema(*a, *c1, *c2, *c) {
                            add(cubic_at(*a, *c1, *c2, *c, t));
                        }
                    }
                }
            }
        }
    }
    (b[0] <= b[2]).then_some(b)
}

fn cubic_at(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2], t: f64) -> [f64; 2] {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    [a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0], a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1]]
}

/// Parameters in (0, 1) where a cubic turns in x or in y.
fn cubic_extrema(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2]) -> Vec<f64> {
    let mut out = Vec::new();
    for i in 0..2 {
        // The derivative's quadratic a·t² + b·t + c.
        let a = 3.0 * (-p0[i] + 3.0 * p1[i] - 3.0 * p2[i] + p3[i]);
        let b = 6.0 * (p0[i] - 2.0 * p1[i] + p2[i]);
        let c = 3.0 * (p1[i] - p0[i]);
        if a.abs() < 1e-12 {
            if b.abs() > 1e-12 {
                out.push(-c / b);
            }
            continue;
        }
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let r = disc.sqrt();
            out.push((-b + r) / (2.0 * a));
            out.push((-b - r) / (2.0 * a));
        }
    }
    out.retain(|t| *t > 0.0 && *t < 1.0);
    out
}

/// The colour table a DGN file without one is drawn with (index 255, the
/// background entry, black).
pub const DGN_DEFAULT_COLORS: [[u8; 3]; 256] = [
    [255, 255, 255], [0, 0, 255], [0, 255, 0], [255, 0, 0], [255, 255, 0], [255, 0, 255], [255, 127, 0], [0, 255, 255],
    [64, 64, 64], [192, 192, 192], [254, 0, 96], [160, 224, 0], [0, 254, 160], [128, 0, 160], [176, 176, 176], [0, 240, 240],
    [240, 240, 240], [0, 0, 240], [0, 240, 0], [240, 0, 0], [240, 240, 0], [240, 0, 240], [240, 122, 0], [0, 240, 240],
    [240, 240, 240], [0, 0, 240], [0, 240, 0], [240, 0, 0], [240, 240, 0], [240, 0, 240], [240, 122, 0], [0, 225, 225],
    [225, 225, 225], [0, 0, 225], [0, 225, 0], [225, 0, 0], [225, 225, 0], [225, 0, 225], [225, 117, 0], [0, 225, 225],
    [225, 225, 225], [0, 0, 225], [0, 225, 0], [225, 0, 0], [225, 225, 0], [225, 0, 225], [225, 117, 0], [0, 210, 210],
    [210, 210, 210], [0, 0, 210], [0, 210, 0], [210, 0, 0], [210, 210, 0], [210, 0, 210], [210, 112, 0], [0, 210, 210],
    [210, 210, 210], [0, 0, 210], [0, 210, 0], [210, 0, 0], [210, 210, 0], [210, 0, 210], [210, 112, 0], [0, 195, 195],
    [195, 195, 195], [0, 0, 195], [0, 195, 0], [195, 0, 0], [195, 195, 0], [195, 0, 195], [195, 107, 0], [0, 195, 195],
    [195, 195, 195], [0, 0, 195], [0, 195, 0], [195, 0, 0], [195, 195, 0], [195, 0, 195], [195, 107, 0], [0, 180, 180],
    [180, 180, 180], [0, 0, 180], [0, 180, 0], [180, 0, 0], [180, 180, 0], [180, 0, 180], [180, 102, 0], [0, 180, 180],
    [180, 180, 180], [0, 0, 180], [0, 180, 0], [180, 0, 0], [180, 180, 0], [180, 0, 180], [180, 102, 0], [0, 165, 165],
    [165, 165, 165], [0, 0, 165], [0, 165, 0], [165, 0, 0], [165, 165, 0], [165, 0, 165], [165, 97, 0], [0, 165, 165],
    [165, 165, 165], [0, 0, 165], [0, 165, 0], [165, 0, 0], [165, 165, 0], [165, 0, 165], [165, 97, 0], [0, 150, 150],
    [150, 150, 150], [0, 0, 150], [0, 150, 0], [150, 0, 0], [150, 150, 0], [150, 0, 150], [150, 92, 0], [0, 150, 150],
    [150, 150, 150], [0, 0, 150], [0, 150, 0], [150, 0, 0], [150, 150, 0], [150, 0, 150], [150, 92, 0], [0, 135, 135],
    [135, 135, 135], [0, 0, 135], [0, 135, 0], [135, 0, 0], [135, 135, 0], [135, 0, 135], [135, 87, 0], [0, 135, 135],
    [135, 135, 135], [0, 0, 135], [0, 135, 0], [135, 0, 0], [135, 135, 0], [135, 0, 135], [135, 87, 0], [0, 120, 120],
    [120, 120, 120], [0, 0, 120], [0, 120, 0], [120, 0, 0], [120, 120, 0], [120, 0, 120], [120, 82, 0], [0, 120, 120],
    [120, 120, 120], [0, 0, 120], [0, 120, 0], [120, 0, 0], [120, 120, 0], [120, 0, 120], [120, 82, 0], [0, 105, 105],
    [105, 105, 105], [0, 0, 105], [0, 105, 0], [105, 0, 0], [105, 105, 0], [105, 0, 105], [105, 77, 0], [0, 105, 105],
    [105, 105, 105], [0, 0, 105], [0, 105, 0], [105, 0, 0], [105, 105, 0], [105, 0, 105], [105, 77, 0], [0, 90, 90],
    [90, 90, 90], [0, 0, 90], [0, 90, 0], [90, 0, 0], [90, 90, 0], [90, 0, 90], [90, 72, 0], [0, 90, 90],
    [90, 90, 90], [0, 0, 90], [0, 90, 0], [90, 0, 0], [90, 90, 0], [90, 0, 90], [90, 72, 0], [0, 75, 75],
    [75, 75, 75], [0, 0, 75], [0, 75, 0], [75, 0, 0], [75, 75, 0], [75, 0, 75], [75, 67, 0], [0, 75, 75],
    [75, 75, 75], [0, 0, 75], [0, 75, 0], [75, 0, 0], [75, 75, 0], [75, 0, 75], [75, 67, 0], [0, 60, 60],
    [60, 60, 60], [0, 0, 60], [0, 60, 0], [60, 0, 0], [60, 60, 0], [60, 0, 60], [60, 62, 0], [0, 60, 60],
    [60, 60, 60], [0, 0, 60], [0, 60, 0], [60, 0, 0], [60, 60, 0], [60, 0, 60], [60, 62, 0], [0, 45, 45],
    [45, 45, 45], [0, 0, 45], [0, 45, 0], [45, 0, 0], [45, 45, 0], [45, 0, 45], [45, 57, 0], [0, 45, 45],
    [45, 45, 45], [0, 0, 45], [0, 45, 0], [45, 0, 0], [45, 45, 0], [45, 0, 45], [45, 57, 0], [0, 30, 30],
    [30, 30, 30], [0, 0, 30], [0, 30, 0], [30, 0, 0], [30, 30, 0], [30, 0, 30], [30, 52, 0], [0, 30, 30],
    [30, 30, 30], [0, 0, 30], [0, 30, 0], [30, 0, 0], [30, 30, 0], [30, 0, 30], [192, 192, 192], [0, 0, 0],
];

/// A colour table element's 256 colours: entries 0-254 from `first`, and
/// entry 255 (the background) from the three bytes that open the table at
/// `first - 3`.
pub fn dgn_color_table(e: &[u8], first: usize) -> Option<Vec<[u8; 3]>> {
    let bg = e.get(first - 3..first)?;
    let body = e.get(first..first + 255 * 3)?;
    let mut out: Vec<[u8; 3]> = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    out.push([bg[0], bg[1], bg[2]]);
    Some(out)
}

/// A line of text the host draws with its own font.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    /// Baseline start, sheet units.
    pub origin: [f64; 2],
    pub height: f64,
    /// Width of a character relative to its height.
    pub width_factor: f64,
    pub rotation: f64,
    pub color: [u8; 3],
    /// The font the text is drawn in: a stroke font name or a TrueType
    /// family.
    pub font: String,
}

/// Height of a TrueType font's capitals per em (Arial and its kin), which
/// turns a plotted em size into a text height.
// ponytail: one ratio for every family; read the font's own metrics if a
// serif family plots noticeably off.
pub const CAP_PER_EM: f64 = 0.716;

/// Points of a B-spline curve: its order, control points and knot vector
/// (clamped: `order` equal knots at each end).
pub fn bspline_points(order: usize, poles: &[[f64; 2]], knots: &[f64], samples: usize) -> Vec<[f64; 2]> {
    let n = poles.len();
    if order < 2 || n < order || knots.len() != n + order {
        return poles.to_vec();
    }
    let (t0, t1) = (knots[order - 1], knots[n]);
    let mut out = Vec::with_capacity(samples + 1);
    for s in 0..=samples {
        let t = t0 + (t1 - t0) * s as f64 / samples as f64;
        // de Boor
        let mut k = order - 1;
        while k < n - 1 && t >= knots[k + 1] {
            k += 1;
        }
        let mut d: Vec<[f64; 2]> = (0..order).map(|j| poles[j + k + 1 - order]).collect();
        for r in 1..order {
            for j in (r..order).rev() {
                let i = j + k + 1 - order;
                let den = knots[i + order - r] - knots[i];
                let a = if den.abs() < 1e-15 { 0.0 } else { (t - knots[i]) / den };
                d[j] = [d[j - 1][0] * (1.0 - a) + d[j][0] * a, d[j - 1][1] * (1.0 - a) + d[j][1] * a];
            }
        }
        out.push(d[order - 1]);
    }
    out
}

#[cfg(test)]
mod arc_cubics_sweep_tests {
    use super::arc_cubics;

    /// The sweep is a raw f64 out of DGN element bytes (`f64_at(e, 112)`):
    /// 1e300 / inf saturate the `as usize` cast to `usize::MAX` and
    /// `Vec::with_capacity` panics `capacity overflow`; ~1e17 would ask
    /// for 2 EB and abort. Legitimate callers top out near 104 rad.
    #[test]
    fn an_enormous_sweep_is_clamped_instead_of_panicking() {
        for sweep in [1e300, f64::INFINITY, -1e300, f64::NEG_INFINITY] {
            let pieces = arc_cubics([0.0; 2], 1.0, 1.0, 0.0, 0.0, sweep);
            assert_eq!(pieces.len(), 4096, "sweep {sweep}");
            assert!(pieces
                .iter()
                .flatten()
                .all(|p| p[0].is_finite() && p[1].is_finite()));
        }
    }

    /// NaN behaves as before: one degenerate piece, no panic.
    #[test]
    fn a_nan_sweep_still_yields_a_single_piece() {
        assert_eq!(arc_cubics([0.0; 2], 1.0, 1.0, 0.0, 0.0, f64::NAN).len(), 1);
    }

    /// Ordinary arcs keep their exact piece count, ceil(|sweep| / 90°).
    #[test]
    fn ordinary_sweeps_keep_their_piece_counts() {
        let expect = |sweep: f64| {
            ((sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1)
        };
        for sweep in [
            0.0,
            1.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::TAU,
            104.1,
            -2.5,
        ] {
            let pieces = arc_cubics([0.0; 2], 3.0, 2.0, 0.3, 0.5, sweep);
            assert_eq!(pieces.len(), expect(sweep), "sweep {sweep}");
        }
    }
}
