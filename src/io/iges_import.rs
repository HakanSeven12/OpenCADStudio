//! IGES reading: the manifold solids (type 186) of a file, their shells,
//! faces, loops and edges (514/510/508/504/502), its trimmed and bounded
//! surfaces (144/142 and 143/141) and its independent bounded surfaces,
//! over spline surfaces and curves (128/126), parametric spline surfaces
//! (114), ruled surfaces (118), surfaces of revolution (120), tabulated
//! cylinders (122), offsets of planes (140), planes, cylinders, cones,
//! spheres and tori (190–198), lines and arcs (110/100) and composite curves
//! (102), as B-rep records.

use std::collections::HashMap;

use ocs_geom::space::{NurbsCurve3, NurbsSurface3, Vec3};
use ocs_solid::geometry::frame;
use ocs_solid::model::{r, Brep};
use solidcore_step::assembly::{Bound, Kind};

use super::brep_build::{self, Builder, EdgeCurve, FaceSpec};
use super::step_import::compose;

/// One parameter: a number, or a Hollerith string.
#[derive(Debug, Clone)]
enum P {
    Num(f64),
    Str(String),
}

impl P {
    fn num(&self) -> f64 {
        match self {
            P::Num(v) => *v,
            P::Str(_) => 0.0,
        }
    }
}

/// Splits a section's text into parameters at `delimiter`, up to the first
/// `terminator`, reading `nH…` strings whole.
fn parameters(text: &str, delimiter: char, terminator: char) -> Vec<P> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        while chars.get(i).is_some_and(|c| *c == ' ') {
            i += 1;
        }
        let start = i;
        while chars.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
        if i > start && matches!(chars.get(i), Some('H' | 'h')) {
            let count: usize = chars[start..i]
                .iter()
                .collect::<String>()
                .parse()
                .unwrap_or(0);
            let body: String = chars.iter().skip(i + 1).take(count).collect();
            i = i.saturating_add(1).saturating_add(count);
            out.push(P::Str(body));
        } else {
            i = start;
            while chars
                .get(i)
                .is_some_and(|c| *c != delimiter && *c != terminator)
            {
                i += 1;
            }
            let raw: String = chars[start..i]
                .iter()
                .collect::<String>()
                .replace(['D', 'd'], "E");
            out.push(P::Num(raw.trim().parse().unwrap_or(0.0)));
        }
        while chars.get(i).is_some_and(|c| *c == ' ') {
            i += 1;
        }
        match chars.get(i) {
            Some(c) if *c == delimiter => i += 1,
            _ => return out,
        }
    }
}

/// A directory entry and its parameters.
struct Entity {
    kind: i32,
    /// The directory entry's form number.
    form: i64,
    /// Pointer to a transformation matrix entity, or zero.
    transform: usize,
    /// Colour number, or a negated pointer to a colour definition.
    color: i64,
    /// The directory entry's level number.
    level: i64,
    /// Whether another entity uses it (the directory's subordinate switch).
    subordinate: bool,
    params: Vec<P>,
}

struct Doc {
    /// Keyed by the entry's first directory line number.
    entities: HashMap<usize, Entity>,
    /// File units to drawing units.
    scale: f64,
    /// Points closer than this are one point, in drawing units.
    resolution: f64,
}

/// A 3×4 matrix applied as `R·p + T`.
type Matrix = super::step_import::Placement;

/// A transformation matrix entity's (124) matrix.
fn matrix_of(entity: &Entity) -> Matrix {
    let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
    [
        [v(1), v(2), v(3), v(4)],
        [v(5), v(6), v(7), v(8)],
        [v(9), v(10), v(11), v(12)],
    ]
}

impl Doc {
    fn get(&self, pointer: f64) -> Option<&Entity> {
        self.entities.get(&(pointer.abs() as usize))
    }

    /// An entry's colour: a numbered one, or a definition's RGB percentages
    /// — as the nearest standard index colour when it is one exactly.
    fn color(&self, number: i64) -> document::types::Color {
        use document::types::Color;
        let rgb = match number {
            // Black becomes the dark grey 250.
            1 => return Color::Index(250),
            // No colour, or a number past the eight standard ones.
            0 => return Color::ByBlock,
            n if n > 8 => return Color::ByBlock,
            2 => [255, 0, 0],
            3 => [0, 255, 0],
            4 => [0, 0, 255],
            5 => [255, 255, 0],
            6 => [255, 0, 255],
            7 => [0, 255, 255],
            8 => [255, 255, 255],
            n if n < 0 => match self.entities.get(&(-n as usize)).filter(|e| e.kind == 314) {
                Some(e) => [1, 2, 3].map(|k| {
                    (e.params.get(k).map_or(0.0, P::num) * 2.55)
                        .round()
                        .clamp(0.0, 255.0) as u8
                }),
                None => return Color::ByLayer,
            },
            _ => return Color::ByLayer,
        };
        match rgb {
            [0, 0, 0] => Color::Index(250),
            [255, 0, 0] => Color::Index(1),
            [255, 255, 0] => Color::Index(2),
            [0, 255, 0] => Color::Index(3),
            [0, 255, 255] => Color::Index(4),
            [0, 0, 255] => Color::Index(5),
            [255, 0, 255] => Color::Index(6),
            [255, 255, 255] => Color::Index(7),
            [r, g, b] => Color::Rgb { r, g, b },
        }
    }

    /// A transformation matrix, with the matrices it chains through its own
    /// transformation pointer applied after it. The chain is cut after a
    /// few links, so matrices that point at each other cannot loop.
    fn matrix(&self, pointer: usize) -> Option<Matrix> {
        let mut entity = self.entities.get(&pointer).filter(|e| e.kind == 124)?;
        let mut m = matrix_of(entity);
        for _ in 0..16 {
            let Some(outer) = (entity.transform != 0)
                .then(|| self.entities.get(&entity.transform))
                .flatten()
                .filter(|e| e.kind == 124)
            else {
                break;
            };
            m = compose(&matrix_of(outer), &m);
            entity = outer;
        }
        Some(m)
    }

    /// `entity`'s placement applied to a point it defines.
    fn place(&self, entity: &Entity) -> impl Fn([f64; 3]) -> [f64; 3] {
        let m = (entity.transform != 0)
            .then(|| self.matrix(entity.transform))
            .flatten();
        let scale = self.scale;
        move |p| {
            let p = match m {
                Some(m) => {
                    [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
                }
                None => p,
            };
            p.map(|c| c * scale)
        }
    }

    fn curve(&self, pointer: f64) -> Option<EdgeCurve> {
        let entity = self.get(pointer)?;
        let p = &entity.params;
        let v = |k: usize| p.get(k).map_or(0.0, P::num);
        let place = self.place(entity);
        Some(match entity.kind {
            110 => EdgeCurve::Line {
                start: place([v(1), v(2), v(3)]),
                end: place([v(4), v(5), v(6)]),
            },
            100 => {
                let centre = place([v(2), v(3), v(1)]);
                let x = place([v(2) + 1.0, v(3), v(1)]);
                let y = place([v(2), v(3) + 1.0, v(1)]);
                let start = place([v(4), v(5), v(1)]);
                let (x, y) = (
                    [x[0] - centre[0], x[1] - centre[1], x[2] - centre[2]],
                    [y[0] - centre[0], y[1] - centre[1], y[2] - centre[2]],
                );
                let normal = [
                    x[1] * y[2] - x[2] * y[1],
                    x[2] * y[0] - x[0] * y[2],
                    x[0] * y[1] - x[1] * y[0],
                ];
                EdgeCurve::Circle {
                    frame: frame(centre, x, normal)?,
                    radius: distance(start, centre),
                }
            }
            126 => {
                // The sizes come from the file: refuse ones its parameters
                // cannot hold.
                let k = v(1) as usize;
                let degree = v(2) as usize;
                let w0 = k.checked_add(degree)?.checked_add(9)?;
                let c0 = w0.checked_add(k)?.checked_add(1)?;
                if c0.checked_add(k.checked_add(1)?.checked_mul(3)?)? > p.len() {
                    return None;
                }
                let knots = (7..w0).map(v).collect();
                let weights = (0..=k).map(|n| v(w0 + n)).collect();
                let points = (0..=k)
                    .map(|n| place([v(c0 + 3 * n), v(c0 + 3 * n + 1), v(c0 + 3 * n + 2)]))
                    .collect();
                EdgeCurve::Nurbs(NurbsCurve3::new(degree, points, knots, Some(weights))?)
            }
            _ => return None,
        })
    }

    /// A boundary curve as its pieces — each a curve and the points it runs
    /// from and to — composite curves taken apart.
    /// Composite curves nest only a few deep, so a deeper (or circular) one
    /// is refused.
    fn pieces(&self, pointer: f64) -> Option<Vec<(EdgeCurve, [f64; 3], [f64; 3])>> {
        self.pieces_at(pointer, 0)
    }

    fn pieces_at(&self, pointer: f64, depth: usize) -> Option<Vec<(EdgeCurve, [f64; 3], [f64; 3])>> {
        let entity = self.get(pointer)?;
        if entity.kind == 102 {
            if depth >= 8 {
                return None;
            }
            let count = entity.params.get(1).map_or(0.0, P::num) as usize;
            let mut out = Vec::new();
            for k in 0..count {
                out.extend(self.pieces_at(entity.params.get(2 + k)?.num(), depth + 1)?);
            }
            return Some(out);
        }
        let curve = self.curve(pointer)?;
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        let place = self.place(entity);
        let (from, to) = match (&curve, entity.kind) {
            (_, 110) => (place([v(1), v(2), v(3)]), place([v(4), v(5), v(6)])),
            (_, 100) => (place([v(4), v(5), v(1)]), place([v(6), v(7), v(1)])),
            (EdgeCurve::Nurbs(nurbs), _) => {
                let (t0, t1) = nurbs.domain();
                (nurbs.point_at_knot(t0), nurbs.point_at_knot(t1))
            }
            _ => return None,
        };
        Some(vec![(curve, from, to)])
    }

    /// The model-space pieces of a curve on a surface (142) or a boundary
    /// (141), each of the latter's curves in the sense it is run.
    fn boundary(&self, pointer: f64) -> Option<Vec<(EdgeCurve, [f64; 3], [f64; 3])>> {
        let entity = self.get(pointer)?;
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        match entity.kind {
            142 if v(4) != 0.0 => self.pieces(v(4)),
            // Only a curve in the surface's parameters: carried onto it.
            142 => self.parameter_pieces(v(3), v(2)),
            141 => {
                let mut out = Vec::new();
                let mut at: usize = 5;
                for _ in 0..(v(4) as usize).min(entity.params.len()) {
                    // A curve given only in the surface's parameters is
                    // carried onto it.
                    let mut pieces = if v(at) != 0.0 {
                        self.pieces(v(at))?
                    } else {
                        let count = (v(at + 2) as usize).min(entity.params.len());
                        let mut pieces = Vec::new();
                        for k in 0..count {
                            pieces.extend(self.parameter_pieces(v(at + 3 + k), v(3))?);
                        }
                        pieces
                    };
                    if v(at + 1) == 2.0 {
                        pieces.reverse();
                        for piece in &mut pieces {
                            std::mem::swap(&mut piece.1, &mut piece.2);
                        }
                    }
                    out.extend(pieces);
                    at = at.saturating_add(3).saturating_add(v(at + 2) as usize);
                }
                Some(out)
            }
            _ => self.pieces(pointer),
        }
    }

    /// A point entity (116), placed.
    fn point_of(&self, pointer: f64) -> Option<[f64; 3]> {
        let entity = self.get(pointer).filter(|e| e.kind == 116)?;
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        Some(self.place(entity)([v(1), v(2), v(3)]))
    }

    /// A direction entity (123), turned by its placement (not moved or
    /// scaled).
    fn direction_of(&self, pointer: f64) -> Option<[f64; 3]> {
        let entity = self.get(pointer).filter(|e| e.kind == 123)?;
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        let d = [v(1), v(2), v(3)];
        Some(
            match (entity.transform != 0)
                .then(|| self.matrix(entity.transform))
                .flatten()
            {
                Some(m) => [0, 1, 2].map(|r| m[r][0] * d[0] + m[r][1] * d[1] + m[r][2] * d[2]),
                None => d,
            },
        )
    }

    /// A curve as a spline: a line, an arc between its ends, or a spline.
    fn nurbs_curve(&self, pointer: f64) -> Option<NurbsCurve3> {
        let entity = self.get(pointer)?;
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        let place = self.place(entity);
        match entity.kind {
            110 => super::iges_surfaces::line(place([v(1), v(2), v(3)]), place([v(4), v(5), v(6)])),
            100 => {
                let centre = place([v(2), v(3), v(1)]);
                let x = place([v(2) + 1.0, v(3), v(1)]);
                let y = place([v(2), v(3) + 1.0, v(1)]);
                let unit = |p: [f64; 3]| {
                    let d = [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]];
                    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    d.map(|c| c / l)
                };
                let (ux, uy) = (unit(x), unit(y));
                let radius = (v(4) - v(2)).hypot(v(5) - v(3)) * self.scale;
                let (a0, a1) = arc_angles(entity);
                super::iges_surfaces::arc(centre, ux, uy, radius, a0, a1)
            }
            _ => match self.curve(pointer)? {
                EdgeCurve::Nurbs(nurbs) => Some(nurbs),
                _ => None,
            },
        }
    }

    /// The sphere or torus a circular arc (100) sweeps round an axis lying
    /// in the arc's plane from `start` radians, framed so its seam (u = 0) is
    /// the spline's; `None` (the spline stands) when the axis leaves that
    /// plane, a sphere's arc crosses the axis, or a torus's arc does not start
    /// at its outermost point (where the torus's own seam runs).
    fn arc_revolution(
        &self,
        arc: f64,
        origin: [f64; 3],
        axis: [f64; 3],
        start: f64,
    ) -> Option<r::Surface> {
        let entity = self.get(arc).filter(|e| e.kind == 100)?;
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        let place = self.place(entity);
        let centre = Vec3::from(place([v(2), v(3), v(1)]));
        let normal = (Vec3::from(place([v(2), v(3), v(1) + 1.0])) - centre).normalize()?;
        let k = Vec3::from(axis).normalize()?;
        let o = Vec3::from(origin);
        let tolerance = self.resolution;
        // The axis must lie in the arc's plane.
        if k.dot(normal).abs() > 1e-9 || (o - centre).dot(normal).abs() > tolerance {
            return None;
        }
        let r = (v(4) - v(2)).hypot(v(5) - v(3));
        let at = |a: f64| Vec3::from(place([v(2) + r * a.cos(), v(3) + r * a.sin(), v(1)]));
        let radial = |p: Vec3| p - (o + k * (p - o).dot(k));
        let (a0, a1) = arc_angles(entity);
        let radius = (at(a0) - centre).length();
        let foot = o + k * (centre - o).dot(k);
        let out = centre - foot;
        let framed =
            |at: Vec3, x: Vec3| frame(at.to_array(), turned(x, k, start).to_array(), k.to_array());
        Some(if out.length() <= tolerance {
            let x = radial(at(0.5 * (a0 + a1))).normalize()?;
            if [a0, a1]
                .into_iter()
                .any(|a| radial(at(a)).dot(x) < -tolerance)
            {
                return None;
            }
            r::Surface::Sphere {
                frame: framed(centre, x)?,
                radius,
            }
        } else {
            let x = out.normalize()?;
            let outermost = centre + x * radius;
            if out.length() <= radius + tolerance || at(a0).distance(outermost) > tolerance {
                return None;
            }
            r::Surface::Torus {
                frame: framed(foot, x)?,
                major_radius: out.length(),
                minor_radius: radius,
                tube_start: 0.0,
            }
        })
    }

    /// The frame of an analytic surface (190–198): its location, its axis
    /// (or normal) and the reference direction of form 1 (else one square
    /// to the axis).
    fn frame(&self, at: f64, axis: Option<f64>, reference: Option<f64>) -> Option<r::Frame> {
        let origin = self.point_of(at)?;
        let axis = match axis {
            Some(a) => self.direction_of(a)?,
            None => [0.0, 0.0, 1.0],
        };
        let x = match reference.and_then(|r| self.direction_of(r)) {
            Some(x) => x,
            None => {
                super::iges_surfaces::any_perpendicular(ocs_geom::space::Vec3::from(axis)).to_array()
            }
        };
        frame(origin, x, axis)
    }

    /// A surface, and the spline whose parameter domain bounds it when the
    /// file trims it with nothing (none for the unbounded analytic ones).
    fn surface_spec(&self, pointer: f64) -> Option<(r::Surface, Option<NurbsSurface3>)> {
        self.surface_spec_at(pointer, 0)
    }

    /// [`Doc::surface_spec`] with offsets (140) of offsets nested `depth`
    /// deep; a deeper (or circular) chain is refused.
    fn surface_spec_at(
        &self,
        pointer: f64,
        depth: usize,
    ) -> Option<(r::Surface, Option<NurbsSurface3>)> {
        use super::iges_surfaces as build;
        let entity = self.get(pointer)?;
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        let form1 = |k: usize| (entity.form == 1).then(|| v(k));
        let spline = |s: NurbsSurface3| Some((brep_build::spline_surface(&s)?, Some(s)));
        let plane = |f: r::Frame| r::Surface::Plane {
            origin: f.origin,
            u_axis: f.x_axis,
            v_axis: f.y_axis,
        };
        match entity.kind {
            128 => spline(self.surface(pointer)?),
            190 => Some((plane(self.frame(v(1), Some(v(2)), form1(3))?), None)),
            // The plane A·x + B·y + C·z = D; a trimmed surface brings its
            // own boundary, so the bounding curve of forms ±1 is not needed.
            108 => {
                let n = [v(1), v(2), v(3)];
                let len2 = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
                if !(len2 > 1e-24) || !v(4).is_finite() {
                    return None;
                }
                let at = n.map(|c| c * v(4) / len2);
                let normal = match (entity.transform != 0)
                    .then(|| self.matrix(entity.transform))
                    .flatten()
                {
                    Some(m) => [0, 1, 2].map(|r| m[r][0] * n[0] + m[r][1] * n[1] + m[r][2] * n[2]),
                    None => n,
                };
                let x = build::any_perpendicular(ocs_geom::space::Vec3::from(normal)).to_array();
                Some((plane(frame(self.place(entity)(at), x, normal)?), None))
            }
            192 => Some((
                r::Surface::Cylinder {
                    frame: self.frame(v(1), Some(v(2)), form1(4))?,
                    radius: v(3) * self.scale,
                },
                None,
            )),
            194 => Some((
                r::Surface::Cone {
                    frame: self.frame(v(1), Some(v(2)), form1(5))?,
                    radius_at_origin: v(3) * self.scale,
                    // The file's cone widens along its axis, as the record's does.
                    half_angle: v(4).to_radians(),
                },
                None,
            )),
            196 => Some((
                r::Surface::Sphere {
                    frame: self.frame(v(1), form1(3), form1(4))?,
                    radius: v(2) * self.scale,
                },
                None,
            )),
            198 => Some((
                r::Surface::Torus {
                    frame: self.frame(v(1), Some(v(2)), form1(5))?,
                    major_radius: v(3) * self.scale,
                    minor_radius: v(4) * self.scale,
                    tube_start: 0.0,
                },
                None,
            )),
            120 => {
                let (o, dir) = self.axis_of(v(1))?;
                let generatrix = self.nurbs_curve(v(2))?;
                let net = build::revolve(&generatrix, o, dir, v(3), v(4))?;
                // A straight generatrix sweeps a cylinder or a cone, a circular
                // arc in a plane through the axis a sphere or a torus (part
                // of a turn too).
                let analytic = match self.get(v(2)).map(|g| g.kind) {
                    Some(110) => {
                        let (t0, t1) = generatrix.domain();
                        let (a, b) = (generatrix.point_at_knot(t0), generatrix.point_at_knot(t1));
                        analytic_revolution(a, b, o, dir, v(3), self.resolution)
                    }
                    Some(100) => self.arc_revolution(v(2), o, dir, v(3)),
                    _ => None,
                };
                match analytic {
                    Some(surface) => Some((surface, Some(net))),
                    None => spline(net),
                }
            }
            122 => {
                let directrix = self.nurbs_curve(v(1))?;
                let start = directrix.point_at_knot(directrix.domain().0);
                let end = self.place(entity)([v(2), v(3), v(4)]);
                spline(build::tabulate(
                    &directrix,
                    [end[0] - start[0], end[1] - start[1], end[2] - start[2]],
                )?)
            }
            118 => {
                // ponytail: the rulings join points at equal relative
                // parameter values (form 1) for form 0 too, which pairs them
                // by equal relative arc length.
                let first = self.nurbs_curve(v(1))?;
                let mut second = self.nurbs_curve(v(2))?;
                if v(3) == 1.0 {
                    second = second.reversed()?;
                }
                spline(build::ruled(&first, &second)?)
            }
            114 => {
                let (m, n) = (v(3) as usize, v(4) as usize);
                // Patch (i, j) is the coefficient block i·(n+1) + j: the file
                // keeps a block for each break point, the last ones unused.
                // The sizes come from the file: refuse ones its parameters
                // cannot hold.
                let blocks = m
                    .checked_sub(1)?
                    .checked_mul(n.checked_add(1)?)?
                    .checked_add(n)?;
                let end = m
                    .checked_add(n)?
                    .checked_add(7)?
                    .checked_add(blocks.checked_mul(48)?)?;
                if n == 0 || end > entity.params.len() {
                    return None;
                }
                let tu: Vec<f64> = (5..=5 + m).map(v).collect();
                let tv: Vec<f64> = (6 + m..=6 + m + n).map(v).collect();
                let at = 7 + m + n;
                let place = self.place(entity);
                let coefficients: Vec<[[f64; 16]; 3]> = (0..m * n)
                    .map(|k| (k / n) * (n + 1) + k % n)
                    .map(|k| {
                        [0, 1, 2]
                            .map(|axis| std::array::from_fn(|c| v(at + 48 * k + 16 * axis + c)))
                    })
                    .collect();
                let raw = build::patches(&tu, &tv, &coefficients)?;
                // The placement applies to the control points.
                let (du, dv) = raw.degrees();
                let (uk, vk) = raw.knots();
                let rows = raw
                    .control_points()
                    .iter()
                    .map(|row| row.iter().map(|p| place(*p)).collect())
                    .collect();
                spline(NurbsSurface3::new(
                    du,
                    dv,
                    rows,
                    uk.to_vec(),
                    vk.to_vec(),
                    None,
                )?)
            }
            140 => {
                // The base pushed along its own normal (u × v): a plane
                // moves, a cylinder, sphere or torus keeps its axis and
                // changes radius; any other base becomes an offset surface.
                if depth >= 8 {
                    return None;
                }
                let (surface, Some(base)) = self.surface_spec_at(v(5), depth + 1)? else {
                    return None;
                };
                let distance = v(4) * self.scale;
                // A base whose normal is the same everywhere (a plane) moves.
                let ((u0, u1), (v0, v1)) = base.domain();
                let normals: Option<Vec<[f64; 3]>> = (0..9)
                    .map(|k| {
                        let (s, t) = ((k % 3) as f64 / 2.0, (k / 3) as f64 / 2.0);
                        base.normal_at_knot(u0 + (u1 - u0) * s, v0 + (v1 - v0) * t)
                    })
                    .collect();
                if let Some(normals) = normals {
                    let n = Vec3::from(normals[0]);
                    if normals.iter().all(|m| Vec3::from(*m).distance(n) <= 1e-12) {
                        return spline(build::translated(&base, (n * distance).to_array())?);
                    }
                }
                let net = offset_surface(&base, distance)?;
                // A point of the offset, to read the new radius off.
                let ((u0, u1), (v0, v1)) = net.domain();
                let p = Vec3::from(net.point_at_knot(0.5 * (u0 + u1), 0.5 * (v0 + v1)));
                let axis = |frame: &r::Frame| {
                    let k = Vec3::from(frame.x_axis).cross(Vec3::from(frame.y_axis));
                    (Vec3::from(frame.origin), k)
                };
                let around = |frame: &r::Frame| {
                    let (o, k) = axis(frame);
                    let q = p - o;
                    (q - k * q.dot(k)).length()
                };
                let offset = match surface {
                    r::Surface::Cylinder { frame, .. } => r::Surface::Cylinder {
                        radius: around(&frame),
                        frame,
                    },
                    r::Surface::Sphere { frame, .. } => r::Surface::Sphere {
                        radius: (p - Vec3::from(frame.origin)).length(),
                        frame,
                    },
                    r::Surface::Torus {
                        frame,
                        major_radius,
                        tube_start,
                        ..
                    } => {
                        let (o, k) = axis(&frame);
                        let q = p - o;
                        let tube = o + (q - k * q.dot(k)).normalize()? * major_radius;
                        r::Surface::Torus {
                            frame,
                            major_radius,
                            minor_radius: (p - tube).length(),
                            tube_start,
                        }
                    }
                    // Any other base's offset is the spline through its points.
                    _ => return spline(net),
                };
                Some((offset, Some(net)))
            }
            _ => None,
        }
    }

    /// The model pieces of a curve given in a surface's parameters (x as u,
    /// y as v): straight where the carried points stay on a line, else a
    /// spline through them.
    fn parameter_pieces(
        &self,
        curve: f64,
        surface: f64,
    ) -> Option<Vec<(EdgeCurve, [f64; 3], [f64; 3])>> {
        let surface_at = self.parameter_surface(surface)?;
        let mut out = Vec::new();
        for piece in self.pieces_unplaced(curve)? {
            let samples = (0..=24)
                .map(|k| {
                    let p = piece_point(&piece, k as f64 / 24.0)?;
                    surface_at(p[0], p[1])
                })
                .collect::<Option<Vec<[f64; 3]>>>()?;
            let (a, b) = (samples[0], samples[24]);
            let straight = samples.iter().all(|p| {
                ocs_geom::space::Vec3::from(*p).distance_to_segment(a.into(), b.into())
                    <= self.resolution
            });
            let curve = if straight {
                EdgeCurve::Line { start: a, end: b }
            } else {
                EdgeCurve::Nurbs(NurbsCurve3::interpolate_fit(
                    &samples,
                    None,
                    None,
                    ocs_geom::space::Parameterization::Chord,
                )?)
            };
            out.push((curve, a, b));
        }
        Some(out)
    }

    /// A surface's point at the parameters the file gives curves on it in:
    /// a spline's (128, 114, and offsets of those) are its knots; a
    /// revolution's (120) the generatrix's own parameter and the angle round
    /// the axis; a tabulated cylinder's (122) and a ruled surface's (118)
    /// their curves' parameters run over 0..1, and 0..1 across.
    fn parameter_surface(
        &self,
        pointer: f64,
    ) -> Option<Box<dyn Fn(f64, f64) -> Option<[f64; 3]> + '_>> {
        let entity = self.get(pointer)?;
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        let net = || match self.surface_spec(pointer)? {
            (_, Some(net)) => Some(net),
            _ => None,
        };
        Some(match entity.kind {
            128 | 114 => {
                let net = net()?;
                Box::new(move |u, v| Some(net.point_at_knot(u, v)))
            }
            140 if matches!(self.get(v(5)).map(|e| e.kind), Some(128 | 114)) => {
                let net = net()?;
                Box::new(move |u, v| Some(net.point_at_knot(u, v)))
            }
            120 => {
                let (o, k) = self.axis_of(v(1))?;
                let (o, k) = (Vec3::from(o), Vec3::from(k).normalize()?);
                let generatrix = v(2);
                Box::new(move |u, angle| {
                    let p = Vec3::from(self.curve_at(generatrix, u)?);
                    let foot = o + k * (p - o).dot(k);
                    Some((foot + turned(p - foot, k, angle)).to_array())
                })
            }
            122 => {
                let directrix = v(1);
                let (t0, t1) = self.curve_range(directrix)?;
                let start = Vec3::from(self.curve_at(directrix, t0)?);
                let along = Vec3::from(self.place(entity)([v(2), v(3), v(4)])) - start;
                Box::new(move |u, v| {
                    let p = Vec3::from(self.curve_at(directrix, t0 + u * (t1 - t0))?);
                    Some((p + along * v).to_array())
                })
            }
            118 => {
                let (first, second, reversed) = (v(1), v(2), v(3) == 1.0);
                let (a0, a1) = self.curve_range(first)?;
                let (b0, b1) = self.curve_range(second)?;
                Box::new(move |u, v| {
                    let w = if reversed { 1.0 - u } else { u };
                    let p = Vec3::from(self.curve_at(first, a0 + u * (a1 - a0))?);
                    let q = Vec3::from(self.curve_at(second, b0 + w * (b1 - b0))?);
                    Some((p + (q - p) * v).to_array())
                })
            }
            _ => return None,
        })
    }

    /// A line's (110) start and its run to its end, placed.
    fn axis_of(&self, pointer: f64) -> Option<([f64; 3], [f64; 3])> {
        let axis = self.get(pointer).filter(|e| e.kind == 110)?;
        let w = |k: usize| axis.params.get(k).map_or(0.0, P::num);
        let place = self.place(axis);
        let (o, e) = (place([w(1), w(2), w(3)]), place([w(4), w(5), w(6)]));
        Some((o, [e[0] - o[0], e[1] - o[1], e[2] - o[2]]))
    }

    /// A curve's point at its parameter as the file defines it: a line's
    /// 0..1, an arc's angle, a spline's knot.
    fn curve_at(&self, pointer: f64, t: f64) -> Option<[f64; 3]> {
        let entity = self.get(pointer)?;
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        let place = self.place(entity);
        match entity.kind {
            110 => Some(place([1, 2, 3].map(|k| v(k) + t * (v(k + 3) - v(k))))),
            100 => {
                let r = (v(4) - v(2)).hypot(v(5) - v(3));
                Some(place([v(2) + r * t.cos(), v(3) + r * t.sin(), v(1)]))
            }
            126 => match self.curve(pointer)? {
                EdgeCurve::Nurbs(nurbs) => Some(nurbs.point_at_knot(t)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The parameters a curve runs over, as [`Doc::curve_at`] reads them.
    fn curve_range(&self, pointer: f64) -> Option<(f64, f64)> {
        let entity = self.get(pointer)?;
        match entity.kind {
            110 => Some((0.0, 1.0)),
            100 => Some(arc_angles(entity)),
            // ponytail: a spline's whole knot range, not the start and end
            // values the file may narrow it to.
            126 => match self.curve(pointer)? {
                EdgeCurve::Nurbs(nurbs) => Some(nurbs.domain()),
                _ => None,
            },
            _ => None,
        }
    }

    /// [`Doc::pieces`] without the drawing scale (parameter-space curves).
    fn pieces_unplaced(&self, pointer: f64) -> Option<Vec<(EdgeCurve, [f64; 3], [f64; 3])>> {
        // ponytail: parameter curves are read through the scaled reader and
        // the scale divided back out; matrices on them are not expected.
        let scale = self.scale;
        Some(
            self.pieces(pointer)?
                .into_iter()
                .map(|(c, a, b)| {
                    (
                        unscale(c, scale),
                        a.map(|x| x / scale),
                        b.map(|x| x / scale),
                    )
                })
                .collect(),
        )
    }

    fn surface(&self, pointer: f64) -> Option<NurbsSurface3> {
        let entity = self.get(pointer)?;
        if entity.kind != 128 {
            return None;
        }
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        let place = self.place(entity);
        let (k1, k2, m1, m2) = (v(1) as usize, v(2) as usize, v(3) as usize, v(4) as usize);
        // The sizes come from the file: refuse ones its parameters cannot
        // hold.
        let s0 = 10;
        let t0 = k1.checked_add(m1)?.checked_add(s0 + 2)?;
        let w0 = t0.checked_add(k2)?.checked_add(m2)?.checked_add(2)?;
        let count = k1.checked_add(1)?.checked_mul(k2.checked_add(1)?)?;
        let c0 = w0.checked_add(count)?;
        if c0.checked_add(count.checked_mul(3)?)? > entity.params.len() {
            return None;
        }
        let u_knots = (s0..t0).map(v).collect();
        let v_knots = (t0..w0).map(v).collect();
        // Points run with the first index fastest.
        let at = |i: usize, j: usize| j * (k1 + 1) + i;
        let weights = (0..=k1)
            .map(|i| (0..=k2).map(|j| v(w0 + at(i, j))).collect())
            .collect();
        let points = (0..=k1)
            .map(|i| {
                (0..=k2)
                    .map(|j| {
                        let n = c0 + 3 * at(i, j);
                        place([v(n), v(n + 1), v(n + 2)])
                    })
                    .collect()
            })
            .collect();
        NurbsSurface3::new(m1, m2, points, u_knots, v_knots, Some(weights))
    }
}

struct Assembly<'a> {
    doc: &'a Doc,
    build: Builder,
    vertices: HashMap<(usize, usize), u32>,
    /// Each edge of an edge list, and whether it runs the file's way.
    edges: HashMap<(usize, usize), (u32, bool)>,
    /// Vertices made where boundary curves meet, by position.
    corners: Vec<([f64; 3], u32)>,
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt()
}

impl<'a> Assembly<'a> {
    fn new(doc: &'a Doc) -> Self {
        Self {
            doc,
            build: Builder::new(),
            vertices: HashMap::new(),
            edges: HashMap::new(),
            corners: Vec::new(),
        }
    }

    /// The tolerance bodies are assembled at: the file's resolution, within
    /// what the assembler takes.
    fn tolerance(&self) -> f64 {
        self.doc.resolution.clamp(1e-6, 1e-2)
    }

    fn vertex(&mut self, list: f64, index: f64) -> Option<u32> {
        let key = (list.abs() as usize, index as usize);
        if let Some(&v) = self.vertices.get(&key) {
            return Some(v);
        }
        let entity = self.doc.get(list).filter(|e| e.kind == 502)?;
        let n = key
            .1
            .checked_sub(1)?
            .checked_mul(3)?
            .checked_add(2)?
            .min(entity.params.len());
        let p = |k: usize| entity.params.get(k).map(P::num);
        let point = self.doc.place(entity)([p(n)?, p(n + 1)?, p(n + 2)?]);
        let v = self.build.vertex(point);
        self.vertices.insert(key, v);
        Some(v)
    }

    fn edge(&mut self, list: f64, index: f64) -> Option<(u32, bool)> {
        let key = (list.abs() as usize, index as usize);
        if let Some(&e) = self.edges.get(&key) {
            return Some(e);
        }
        let entity = self.doc.get(list).filter(|e| e.kind == 504)?;
        let n = key
            .1
            .checked_sub(1)?
            .checked_mul(5)?
            .checked_add(2)?
            .min(entity.params.len());
        let p = |k: usize| entity.params.get(k).map(P::num);
        let curve = self.doc.curve(p(n)?)?;
        let start = self.vertex(p(n + 1)?, p(n + 2)?)?;
        let end = self.vertex(p(n + 3)?, p(n + 4)?)?;
        let e = self.build.edge(curve, start, end)?;
        self.edges.insert(key, e);
        Some(e)
    }

    /// The vertex at a point, shared with one already there.
    fn corner(&mut self, point: [f64; 3]) -> u32 {
        let tolerance = self.doc.resolution;
        if let Some((_, v)) = self
            .corners
            .iter()
            .find(|(p, _)| distance(*p, point) <= tolerance)
        {
            return *v;
        }
        let v = self.build.vertex(point);
        self.corners.push((point, v));
        v
    }

    /// A loop along curve pieces, in their order.
    fn curve_ring(
        &mut self,
        pieces: Vec<(EdgeCurve, [f64; 3], [f64; 3])>,
    ) -> Option<Vec<(u32, bool)>> {
        // Pieces of one loop follow on from each other: a piece starts where
        // the one before it ends (and the last runs back to the first) even
        // when the file's points part by more than its resolution, up to a
        // ten-thousandth of the loop's size.
        let (lo, hi) = pieces.iter().flat_map(|(_, f, t)| [*f, *t]).fold(
            ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(lo, hi), p| {
                (
                    [0, 1, 2].map(|k| lo[k].min(p[k])),
                    [0, 1, 2].map(|k| hi[k].max(p[k])),
                )
            },
        );
        let gap = (1e-4 * distance(lo, hi)).max(self.doc.resolution);
        let mut ring = Vec::new();
        let mut first: Option<([f64; 3], u32)> = None;
        let mut last: Option<([f64; 3], u32)> = None;
        let count = pieces.len();
        for (k, (curve, from, to)) in pieces.into_iter().enumerate() {
            if distance(from, to) <= self.doc.resolution
                && matches!(curve, EdgeCurve::Line { .. })
            {
                continue;
            }
            let a = match last {
                Some((point, v)) if distance(point, from) <= gap => v,
                _ => self.corner(from),
            };
            let b = match first {
                Some((point, v)) if k + 1 == count && distance(point, to) <= gap => v,
                _ => self.corner(to),
            };
            first.get_or_insert((from, a));
            last = Some((to, b));
            ring.push(self.build.edge(curve, a, b)?);
        }
        (!ring.is_empty()).then_some(ring)
    }

    /// A loop round the edge of a spline surface's parameter domain; sides
    /// that close up to a point (a pole) are left out, and a side that runs
    /// back along the opposite one (the surface closes up there, a seam) is
    /// that side's edge used the other way. Also whether every side closed
    /// up so (the sheet bounds a volume).
    fn domain_ring(&mut self, surface: &NurbsSurface3) -> Option<(Vec<(u32, bool)>, bool)> {
        let ((u0, u1), (v0, v1)) = surface.domain();
        let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
        let sides = [(1, v0), (0, u1), (1, v1), (0, u0)];
        let tolerance = self.doc.resolution;
        let mut traced: Vec<Option<([f64; 3], [f64; 3], [f64; 3], (u32, bool))>> = Vec::new();
        let mut ring = Vec::new();
        for (k, (axis, at)) in sides.into_iter().enumerate() {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let (from, to) = (
                surface.point_at_knot(a.0, a.1),
                surface.point_at_knot(b.0, b.1),
            );
            let middle = surface.point_at_knot(0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1));
            if distance(from, middle) <= tolerance && distance(to, middle) <= tolerance {
                traced.push(None);
                continue;
            }
            let seam = (k >= 2)
                .then(|| traced[k - 2])
                .flatten()
                .filter(|(f, t, m, _)| {
                    distance(*f, to) <= tolerance
                        && distance(*t, from) <= tolerance
                        && distance(*m, middle) <= tolerance
                });
            if let Some((.., (edge, along))) = seam {
                ring.push((edge, !along));
                traced[k - 2] = None;
                traced.push(None);
                continue;
            }
            let (p, q) = (self.corner(from), self.corner(to));
            let edge = self
                .build
                .edge(EdgeCurve::Nurbs(surface.isocurve(axis, at)?), p, q)?;
            ring.push(edge);
            traced.push(Some((from, to, middle, edge)));
        }
        // Closed when no side is left without its seam partner.
        let closed = traced.iter().all(Option::is_none);
        (!ring.is_empty()).then_some((ring, closed))
    }

    /// A trimmed (144) or bounded (143) surface as a one-face sheet.
    fn sheet(&mut self, entity: &Entity) -> Option<FaceSpec> {
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        // A count can be no more than the parameters there are.
        let count = (v(3) as usize).min(entity.params.len());
        let (surface_at, outer, holes): (f64, Option<f64>, Vec<f64>) = match entity.kind {
            144 => (
                v(1),
                (v(2) != 0.0).then(|| v(4)),
                (0..count).map(|k| v(5 + k)).collect(),
            ),
            143 => (
                v(2),
                (count > 0).then(|| v(4)),
                (1..count).map(|k| v(4 + k)).collect(),
            ),
            _ => return None,
        };
        let (surface, net) = self.doc.surface_spec(surface_at)?;
        let mut first = match outer {
            Some(boundary) => {
                let pieces = self.doc.boundary(boundary)?;
                self.curve_ring(pieces)?
            }
            None => self.domain_ring(net.as_ref()?)?.0,
        };
        self.build.orient(&surface, &mut first, true);
        let mut loops = vec![Bound::Edges(first)];
        for hole in holes {
            let pieces = self.doc.boundary(hole)?;
            let mut ring = self.curve_ring(pieces)?;
            self.build.orient(&surface, &mut ring, false);
            loops.push(Bound::Edges(ring));
        }
        Some(FaceSpec {
            surface,
            forward: true,
            loops,
        })
    }

    /// A loop of a face (508): its edges, each with whether the loop runs it
    /// the file's way, or the one vertex it is.
    fn ring(&mut self, pointer: f64) -> Option<Bound> {
        let entity = self.doc.get(pointer).filter(|e| e.kind == 508)?;
        let p = |k: usize| entity.params.get(k).map(P::num);
        let mut ring = Vec::new();
        let mut lone = None;
        let mut at = 2;
        for _ in 0..p(1)? as usize {
            let (kind, list, index, agrees, pcurves) =
                (p(at)?, p(at + 1)?, p(at + 2)?, p(at + 3)?, p(at + 4)?);
            if kind == 0.0 {
                let (edge, along) = self.edge(list, index)?;
                ring.push((edge, (agrees != 0.0) == along));
            } else if lone.is_none() {
                lone = self.vertex(list, index);
            }
            at = at
                .saturating_add(5)
                .saturating_add((pcurves as usize).saturating_mul(2));
        }
        Some(match lone {
            Some(vertex) if ring.is_empty() => Bound::Vertex(vertex),
            _ => Bound::Edges(ring),
        })
    }

    /// An independent bounded surface as a one-face sheet over its whole
    /// parameter domain, and whether the face closes up on itself all round
    /// (a whole sphere or torus), facing out of what it encloses.
    fn whole(&mut self, pointer: f64) -> Option<(FaceSpec, bool)> {
        let (surface, net) = self.doc.surface_spec(pointer)?;
        let net = net?;
        let (mut ring, closed) = self.domain_ring(&net)?;
        self.build.orient(&surface, &mut ring, true);
        let forward = !closed || (enclosed(&net) > 0.0) == brep_build::agrees(&surface, &net);
        Some((
            FaceSpec {
                surface,
                forward,
                loops: vec![Bound::Edges(ring)],
            },
            closed,
        ))
    }

    fn face(&mut self, pointer: f64, agrees: bool) -> Option<FaceSpec> {
        let entity = self.doc.get(pointer).filter(|e| e.kind == 510)?;
        let p = |k: usize| entity.params.get(k).map(P::num);
        let (surface, net) = self.doc.surface_spec(p(1)?)?;
        let mut loops = (0..p(2)? as usize)
            .map(|n| self.ring(p(4 + n)?))
            .collect::<Option<Vec<_>>>()?;
        // A vertex loop bounds nothing; only a sphere, cone or torus keeps
        // one, as the point its surface closes to.
        let poles = matches!(
            surface,
            r::Surface::Sphere { .. } | r::Surface::Cone { .. } | r::Surface::Torus { .. }
        );
        loops.retain(|bound| match bound {
            Bound::Edges(uses) => !uses.is_empty(),
            Bound::Vertex(_) => poles,
        });
        // Loops run counter-clockwise round the normal of the surface the
        // file names; an analytic surface read in place of its spline may
        // have the other.
        let same = net
            .as_ref()
            .is_none_or(|net| brep_build::agrees(&surface, net));
        if !same {
            for bound in &mut loops {
                if let Bound::Edges(uses) = bound {
                    uses.reverse();
                    for (_, forward) in uses.iter_mut() {
                        *forward = !*forward;
                    }
                }
            }
        }
        Some(FaceSpec {
            surface,
            forward: agrees == same,
            loops,
        })
    }

    fn shell(&mut self, pointer: f64, agrees: bool) -> Option<Vec<FaceSpec>> {
        let entity = self.doc.get(pointer).filter(|e| e.kind == 514)?;
        let p = |k: usize| entity.params.get(k).map(P::num);
        (0..p(1)? as usize)
            .map(|n| {
                let face_agrees = p(3 + 2 * n)? != 0.0;
                self.face(p(2 + 2 * n)?, face_agrees == agrees)
            })
            .collect()
    }
}

/// What an IGES file holds.
pub struct IgesModel {
    /// Each solid, scaled to the drawing, with its colour and layer.
    pub bodies: Vec<(Brep, document::types::Color, String)>,
    /// Each trimmed or bounded surface, a one-face sheet, with its colour
    /// and layer.
    pub sheets: Vec<(Brep, document::types::Color, String)>,
    /// Each spline curve no other entity uses, with its colour and layer.
    pub curves: Vec<(NurbsCurve3, document::types::Color, String)>,
}

/// `drawing_mm`: millimetres per drawing unit, `None` for a unitless drawing.
pub fn read(bytes: &[u8], drawing_mm: Option<f64>) -> Option<IgesModel> {
    let text = String::from_utf8_lossy(bytes);
    let mut sections: HashMap<char, Vec<&str>> = HashMap::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.len() < 73 {
            continue;
        }
        let Some(section) = line.get(72..73).and_then(|s| s.chars().next()) else {
            continue;
        };
        sections.entry(section).or_default().push(line);
    }
    let directory = sections.get(&'D')?;
    let global: String = sections
        .get(&'G')
        .map(|lines| lines.iter().filter_map(|l| l.get(..72)).collect())
        .unwrap_or_default();
    // The delimiters are themselves the first two parameters, when given.
    let mut delimiter = ',';
    let mut terminator = ';';
    if global.starts_with("1H") {
        delimiter = global.chars().nth(2).unwrap_or(',');
        let rest = global.get(4..).unwrap_or_default();
        if rest.starts_with("1H") {
            terminator = rest.chars().nth(2).unwrap_or(';');
        }
    }
    let globals = parameters(&global, delimiter, terminator);
    let unit_mm = match globals.get(13).map(P::num).unwrap_or(1.0) as i32 {
        1 => 25.4,
        4 => 304.8,
        5 => 1_609_344.0,
        6 => 1000.0,
        7 => 1_000_000.0,
        8 => 0.0254,
        9 => 0.001,
        10 => 10.0,
        11 => 0.000_025_4,
        3 => match globals.get(14) {
            Some(P::Str(name))
                if name.eq_ignore_ascii_case("IN") || name.eq_ignore_ascii_case("INCH") =>
            {
                25.4
            }
            Some(P::Str(name)) if name.eq_ignore_ascii_case("FT") => 304.8,
            Some(P::Str(name)) if name.eq_ignore_ascii_case("M") => 1000.0,
            Some(P::Str(name)) if name.eq_ignore_ascii_case("CM") => 10.0,
            _ => 1.0,
        },
        _ => 1.0,
    } * globals
        .get(12)
        .map(P::num)
        .filter(|s| *s > 0.0)
        .map_or(1.0, |s| 1.0 / s);

    let parameter_lines: HashMap<usize, &str> = sections
        .get(&'P')
        .map(|lines| {
            lines
                .iter()
                .filter_map(|l| Some((l.get(73..)?.trim().parse().ok()?, *l)))
                .collect()
        })
        .unwrap_or_default();
    let field = |line: &str, n: usize| -> i64 {
        line.get(n * 8..n * 8 + 8)
            .and_then(|f| f.trim().parse().ok())
            .unwrap_or(0)
    };
    let mut entities = HashMap::new();
    for pair in directory.chunks(2) {
        let [first, second] = pair else { continue };
        let Some(number) = first.get(73..).and_then(|n| n.trim().parse::<usize>().ok()) else {
            continue;
        };
        // An entry spans at most every parameter line there is.
        let start = field(first, 1).max(0) as usize;
        let count = (field(second, 3).max(1) as usize).min(parameter_lines.len());
        let data: String = (start..start.saturating_add(count))
            .filter_map(|n| parameter_lines.get(&n))
            .filter_map(|l| l.get(..64))
            .collect();
        entities.insert(
            number,
            Entity {
                kind: field(first, 0) as i32,
                form: field(second, 4),
                transform: field(first, 6).max(0) as usize,
                color: field(second, 2),
                level: field(first, 4),
                subordinate: (field(first, 8) / 10_000) % 100 != 0,
                params: parameters(&data, delimiter, terminator),
            },
        );
    }
    // Level names: a level function property (406 form 3) names the level
    // its own entry is on.
    let level_names: HashMap<i64, String> = entities
        .values()
        .filter(|e| e.kind == 406 && e.form == 3)
        .filter_map(|e| match e.params.get(3) {
            Some(P::Str(name)) if !name.is_empty() => Some((e.level, name.clone())),
            _ => None,
        })
        .collect();
    // An entity's layer: its level's name, else the level number; level 0
    // (and a pointer to several levels) is layer 0.
    let layer = |entity: &Entity| match entity.level {
        n if n > 0 => level_names
            .get(&n)
            .cloned()
            .unwrap_or_else(|| n.to_string()),
        _ => "0".to_string(),
    };
    let scale = drawing_mm
        .filter(|mm| *mm > 0.0)
        .map_or(1.0, |mm| unit_mm / mm);
    let resolution = globals
        .get(18)
        .map(P::num)
        .filter(|r| *r > 0.0)
        .unwrap_or(1e-6)
        * scale;
    let doc = Doc {
        entities,
        scale,
        resolution,
    };

    let mut solids: Vec<usize> = doc
        .entities
        .iter()
        .filter(|(_, e)| e.kind == 186)
        .map(|(n, _)| *n)
        .collect();
    solids.sort_unstable();
    let mut bodies = Vec::new();
    for number in solids {
        let entity = &doc.entities[&number];
        let v = |k: usize| entity.params.get(k).map(P::num);
        let Some((shell, agrees)) = v(1).zip(v(2)) else {
            continue;
        };
        let mut assembly = Assembly::new(&doc);
        let mut shells = Vec::new();
        let outer = assembly.shell(shell, agrees != 0.0);
        let voids = (0..v(3).unwrap_or(0.0) as usize)
            .map(|n| assembly.shell(v(4 + 2 * n)?, v(5 + 2 * n)? != 0.0))
            .collect::<Option<Vec<_>>>();
        if let (Some(outer), Some(voids)) = (outer, voids) {
            shells.push(outer);
            shells.extend(voids);
            let kind = if shells.len() == 1 { Kind::Solid } else { Kind::Voids };
            let tolerance = assembly.tolerance();
            let body = assembly.build.body(&shells, kind, tolerance).ok().or_else(|| {
                // Faces whose boundaries meet without sharing their edges
                // are sewn into one shell.
                let model = assembly.build.body(&shells, Kind::Model, tolerance).ok()?;
                ocs_solid::model::sewn(&[&model], tolerance).ok()
            });
            if let Some(body) = body {
                bodies.push((body, doc.color(entity.color), layer(entity)));
            }
        }
    }
    // The solids are listed last first.
    bodies.reverse();
    let mut numbers: Vec<usize> = doc
        .entities
        .iter()
        .filter(|(_, e)| {
            matches!(e.kind, 143 | 144)
                || (!e.subordinate && matches!(e.kind, 114 | 118 | 120 | 122 | 128 | 140))
        })
        .map(|(n, _)| *n)
        .collect();
    numbers.sort_unstable();
    let mut sheets = Vec::new();
    for number in numbers {
        let entity = &doc.entities[&number];
        let mut assembly = Assembly::new(&doc);
        let made = match entity.kind {
            143 | 144 => assembly.sheet(entity).map(|face| (face, false)),
            _ => assembly.whole(number as f64),
        };
        let Some((face, closed)) = made else {
            continue;
        };
        let tolerance = assembly.tolerance();
        let shells = [vec![face]];
        let solid = closed
            .then(|| assembly.build.body(&shells, Kind::Solid, tolerance).ok())
            .flatten();
        let attributes = (doc.color(entity.color), layer(entity));
        if let Some(body) = solid {
            bodies.push((body, attributes.0, attributes.1));
        } else if let Ok(body) = assembly.build.body(&shells, Kind::Model, tolerance) {
            sheets.push((body, attributes.0, attributes.1));
        }
    }
    // Rational B-spline curves (126) no other entity uses come in as
    // splines.
    let mut numbers: Vec<usize> = doc
        .entities
        .iter()
        .filter(|(_, e)| e.kind == 126 && !e.subordinate)
        .map(|(n, _)| *n)
        .collect();
    numbers.sort_unstable();
    let curves = numbers
        .into_iter()
        .filter_map(|number| {
            let entity = &doc.entities[&number];
            Some((
                doc.nurbs_curve(number as f64)?,
                doc.color(entity.color),
                layer(entity),
            ))
        })
        .collect();
    Some(IgesModel {
        bodies,
        sheets,
        curves,
    })
}

/// A point along a parameter-space curve piece at `t` in 0..=1; an arc
/// runs counter-clockwise from the piece's start to its end.
fn piece_point((curve, from, to): &(EdgeCurve, [f64; 3], [f64; 3]), t: f64) -> Option<[f64; 3]> {
    Some(match curve {
        EdgeCurve::Line { start, end } => [0, 1, 2].map(|k| start[k] + (end[k] - start[k]) * t),
        EdgeCurve::Nurbs(n) => {
            let (a, b) = n.domain();
            n.point_at_knot(a + (b - a) * t)
        }
        EdgeCurve::Circle { frame, radius } => {
            let (o, x, y) = (
                Vec3::from(frame.origin),
                Vec3::from(frame.x_axis),
                Vec3::from(frame.y_axis),
            );
            let angle = |p: [f64; 3]| {
                let q = Vec3::from(p) - o;
                q.dot(y).atan2(q.dot(x))
            };
            let (a0, mut a1) = (angle(*from), angle(*to));
            while a1 <= a0 + 1e-12 {
                a1 += std::f64::consts::TAU;
            }
            let a = a0 + (a1 - a0) * t;
            (o + (x * a.cos() + y * a.sin()) * *radius).to_array()
        }
    })
}

/// A curve read with the drawing scale, back in file units.
fn unscale(curve: EdgeCurve, scale: f64) -> EdgeCurve {
    match curve {
        EdgeCurve::Line { start, end } => EdgeCurve::Line {
            start: start.map(|c| c / scale),
            end: end.map(|c| c / scale),
        },
        EdgeCurve::Nurbs(n) => {
            let points = n
                .control_points()
                .iter()
                .map(|p| p.map(|c| c / scale))
                .collect();
            NurbsCurve3::new(
                n.degree(),
                points,
                n.knots().to_vec(),
                Some(n.weights().to_vec()),
            )
            .map(EdgeCurve::Nurbs)
            .unwrap_or(EdgeCurve::Nurbs(n))
        }
        EdgeCurve::Circle { mut frame, radius } => {
            frame.origin = frame.origin.map(|x| x / scale);
            EdgeCurve::Circle {
                frame,
                radius: radius / scale,
            }
        }
    }
}

/// The cylinder or cone a straight generatrix from `a` to `b` sweeps round
/// an axis from `start` radians, framed so its seam (u = 0) is the spline's;
/// `None` (the spline stands) when the line misses the axis's plane, crosses
/// the axis or runs square to it.
fn analytic_revolution(
    a: [f64; 3],
    b: [f64; 3],
    origin: [f64; 3],
    axis: [f64; 3],
    start: f64,
    tolerance: f64,
) -> Option<r::Surface> {
    let k = Vec3::from(axis).normalize()?;
    let (a, b, o) = (Vec3::from(a), Vec3::from(b), Vec3::from(origin));
    let skew = (b - a).cross(k);
    if (a - o).dot(skew).abs() > tolerance * skew.length() {
        return None;
    }
    let foot = |p: Vec3| o + k * (p - o).dot(k);
    // Framed at the end farther from the axis, whose side the line keeps to.
    let a_is_far = (a - foot(a)).length() >= (b - foot(b)).length();
    let (far, near) = if a_is_far { (a, b) } else { (b, a) };
    let x = (far - foot(far)).normalize()?;
    let (r_far, r_near) = ((far - foot(far)).length(), (near - foot(near)).dot(x));
    let along = (near - far).dot(k);
    if r_near < -tolerance || along.abs() <= tolerance {
        return None;
    }
    let x_axis = turned(x, k, start).to_array();
    let frame = frame(foot(far).to_array(), x_axis, k.to_array())?;
    Some(if (r_far - r_near).abs() <= tolerance {
        r::Surface::Cylinder {
            frame,
            radius: r_far,
        }
    } else {
        r::Surface::Cone {
            frame,
            radius_at_origin: r_far,
            half_angle: ((r_near - r_far) / along).atan(),
        }
    })
}

/// `x` turned by `angle` radians round the unit axis `k`.
fn turned(x: Vec3, k: Vec3, angle: f64) -> Vec3 {
    x * angle.cos() + k.cross(x) * angle.sin()
}

/// A circular arc's (100) start and end angles in its own plane, the end
/// past the start (a whole turn when they meet).
fn arc_angles(entity: &Entity) -> (f64, f64) {
    let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
    let a0 = (v(5) - v(3)).atan2(v(4) - v(2));
    let mut a1 = (v(7) - v(3)).atan2(v(6) - v(2));
    while a1 <= a0 + 1e-12 {
        a1 += std::f64::consts::TAU;
    }
    (a0, a1)
}

/// The volume a closed spline's natural normal sweeps, a third of the flux
/// of the position through it: positive when that normal faces out.
fn enclosed(net: &NurbsSurface3) -> f64 {
    const N: usize = 32;
    let ((u0, u1), (v0, v1)) = net.domain();
    let (du, dv) = ((u1 - u0) / N as f64, (v1 - v0) / N as f64);
    let mut sum = 0.0;
    for i in 0..N {
        for j in 0..N {
            let (u, v) = (u0 + du * (i as f64 + 0.5), v0 + dv * (j as f64 + 0.5));
            if let Some((su, sv)) = net.tangents_at_knot(u, v) {
                let p = Vec3::from(net.point_at_knot(u, v));
                sum += p.dot(Vec3::from(su).cross(Vec3::from(sv)));
            }
        }
    }
    sum * du * dv / 3.0
}

/// A spline standing for the offset of `base`: points pushed `distance`
/// along the base's normal on a grid, interpolated row by row and then
/// column by column. Evenly spaced samples interpolated at even parameters
/// keep the base's `(u, v)`, so curves given in them still apply.
fn offset_surface(base: &NurbsSurface3, distance: f64) -> Option<NurbsSurface3> {
    let ((u0, u1), (v0, v1)) = base.domain();
    let rows = base.control_points();
    let (u_count, v_count) = ((rows.len() * 4).max(16), (rows.first()?.len() * 4).max(4));
    let at = |i: usize, count: usize, low: f64, high: f64| low + (high - low) * i as f64 / count as f64;
    let grid = (0..=u_count)
        .map(|i| {
            let u = at(i, u_count, u0, u1);
            (0..=v_count)
                .map(|j| {
                    let v = at(j, v_count, v0, v1);
                    let point = Vec3::from(base.point_at_knot(u, v));
                    let normal = Vec3::from(base.normal_at_knot(u, v)?);
                    Some((point + normal * distance).to_array())
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    let [u_closed, v_closed] = base.periodicity();
    Some(interpolate_grid(&grid, (u0, u1), (v0, v1))?.with_periodicity(u_closed, v_closed))
}

/// A cubic surface through an evenly spaced grid of points (`grid[i][j]`, i
/// along `u`), interpolated along `v` and then along `u`, its knots put on
/// the given ranges.
fn interpolate_grid(
    grid: &[Vec<[f64; 3]>],
    (u0, u1): (f64, f64),
    (v0, v1): (f64, f64),
) -> Option<NurbsSurface3> {
    let fit = |points: &[[f64; 3]]| {
        NurbsCurve3::interpolate_fit(points, None, None, ocs_geom::space::Parameterization::Uniform)
    };
    let along_v = grid.iter().map(|row| fit(row)).collect::<Option<Vec<_>>>()?;
    let v_knots = along_v.first()?.knots().to_vec();
    // Each column of the rows' control points, fitted along u.
    let mut columns = Vec::new();
    let mut u_knots = Vec::new();
    for j in 0..along_v.first()?.control_points().len() {
        let column: Vec<[f64; 3]> = along_v
            .iter()
            .map(|row| row.control_points().get(j).copied())
            .collect::<Option<_>>()?;
        let fitted = fit(&column)?;
        u_knots = fitted.knots().to_vec();
        columns.push(fitted.control_points().to_vec());
    }
    let points: Vec<Vec<[f64; 3]>> = (0..columns.first()?.len())
        .map(|i| columns.iter().map(|column| column[i]).collect())
        .collect();
    let rescale = |knots: Vec<f64>, low: f64, high: f64| -> Vec<f64> {
        let (first, last) = (knots[0], knots[knots.len() - 1]);
        knots
            .iter()
            .map(|knot| low + (knot - first) / (last - first) * (high - low))
            .collect()
    };
    let weights = vec![vec![1.0; points.first()?.len()]; points.len()];
    NurbsSurface3::new_strict(
        3,
        3,
        points,
        rescale(u_knots, u0, u1),
        rescale(v_knots, v0, v1),
        weights,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: &[u8] = include_bytes!("../../tests/fixtures/iges/box.igs");
    const CYLINDER: &[u8] = include_bytes!("../../tests/fixtures/iges/cylinder.igs");
    const INCH_CUBE: &[u8] = include_bytes!("../../tests/fixtures/iges/inch_cube.igs");
    const SPLINE: &[u8] = include_bytes!("../../tests/fixtures/iges/spline.igs");
    const ATTRIBUTES: &[u8] = include_bytes!("../../tests/fixtures/iges/attributes.igs");
    const RATIONAL_ARC: &[u8] = include_bytes!("../../tests/fixtures/iges/rational_arc.igs");
    const TWO_SOLIDS: &[u8] = include_bytes!("../../tests/fixtures/iges/two_solids.igs");
    const PLANE_108: &[u8] = include_bytes!("../../tests/fixtures/iges/plane108.igs");
    const LARGE_REVOLUTION: &[u8] =
        include_bytes!("../../tests/fixtures/iges/revolution_large_angle.igs");

    fn bounds(body: &Brep) -> ([f64; 3], [f64; 3]) {
        let b = ocs_solid::model::aabb(body).expect("a body with geometry");
        (b.min, b.max)
    }

    fn assert_close(actual: [f64; 3], expected: [f64; 3], tolerance: f64, what: &str) {
        for k in 0..3 {
            assert!(
                (actual[k] - expected[k]).abs() <= tolerance,
                "{what}: {actual:?} != {expected:?}"
            );
        }
    }

    /// The volume a closed body encloses.
    fn volume(body: &Brep) -> f64 {
        ocs_solid::model::volume(body).expect("a body Solidcore opens")
    }

    /// The one solid a file holds, read into a millimetre drawing.
    fn one_solid(bytes: &[u8]) -> Brep {
        let model = read(bytes, Some(1.0)).expect("an IGES model");
        assert_eq!(
            (model.bodies.len(), model.sheets.len(), model.curves.len()),
            (1, 0, 0)
        );
        model.bodies.into_iter().next().unwrap().0
    }

    #[test]
    fn a_box_solid_reads_as_one_closed_body_of_six_faces() {
        let body = one_solid(BOX);
        assert_eq!(body.faces.len(), 6);
        let (min, max) = bounds(&body);
        assert_close(min, [0.0, 0.0, 0.0], 1e-6, "min");
        assert_close(max, [10.0, 20.0, 30.0], 1e-6, "max");
        // A positive volume means outward faces.
        assert!((volume(&body) - 6000.0).abs() < 1e-6, "{}", volume(&body));
    }

    #[test]
    fn a_cylinder_solid_reads_with_its_curved_face() {
        let body = one_solid(CYLINDER);
        assert_eq!(body.faces.len(), 3, "the wall and two caps");
        let (min, max) = bounds(&body);
        assert_close(min, [-5.0, -5.0, 0.0], 1e-6, "min");
        assert_close(max, [5.0, 5.0, 10.0], 1e-6, "max");
        // Within 1 % of 250π.
        let exact = 250.0 * std::f64::consts::PI;
        let v = volume(&body);
        assert!((v - exact).abs() < exact * 0.01, "{v} != {exact}");
    }

    #[test]
    fn file_units_are_scaled_to_the_drawing_units() {
        let size = |bytes: &[u8], drawing_mm| {
            let model = read(bytes, drawing_mm).expect("an IGES model");
            let (min, max) = bounds(&model.bodies[0].0);
            [0, 1, 2].map(|k| max[k] - min[k])
        };
        // A cube written in inches: 25.4 mm, 1 inch, and 1 file unit in a
        // drawing without units.
        assert_close(size(INCH_CUBE, Some(1.0)), [25.4; 3], 1e-6, "mm drawing");
        assert_close(size(INCH_CUBE, Some(25.4)), [1.0; 3], 1e-9, "inch drawing");
        assert_close(size(INCH_CUBE, None), [1.0; 3], 1e-9, "unitless drawing");
        // A box written in millimetres.
        assert_close(
            size(BOX, Some(25.4)),
            [10.0 / 25.4, 20.0 / 25.4, 30.0 / 25.4],
            1e-9,
            "inch drawing",
        );
        assert_close(
            size(BOX, Some(1000.0)),
            [0.01, 0.02, 0.03],
            1e-12,
            "metre drawing",
        );
        assert_close(
            size(BOX, None),
            [10.0, 20.0, 30.0],
            1e-9,
            "unitless drawing",
        );
    }

    #[test]
    fn a_free_spline_curve_reads_through_its_interpolated_points() {
        let model = read(SPLINE, Some(1.0)).expect("an IGES model");
        assert_eq!(
            (model.bodies.len(), model.sheets.len(), model.curves.len()),
            (0, 0, 1)
        );
        let (curve, color, layer) = &model.curves[0];
        assert_eq!(curve.degree(), 3);
        assert_eq!(
            (*color, layer.as_str()),
            (document::types::Color::ByBlock, "0")
        );
        let (t0, t1) = curve.domain();
        let samples: Vec<[f64; 3]> = (0..=4000)
            .map(|k| curve.point_at_knot(t0 + (t1 - t0) * k as f64 / 4000.0))
            .collect();
        assert_close(samples[0], [0.0, 0.0, 0.0], 1e-9, "start");
        assert_close(samples[4000], [30.0, 8.0, 0.0], 1e-9, "end");
        for through in [[10.0, 5.0, 0.0], [20.0, 0.0, 3.0]] {
            let nearest = samples
                .iter()
                .map(|p| distance(*p, through))
                .fold(f64::INFINITY, f64::min);
            assert!(nearest < 0.01, "passes {through:?}: {nearest}");
        }
        // Between them the curve overshoots; FreeCAD, sampling the same
        // curve at 4,001 points, finds y down to -0.4543 and z from -0.8272
        // to 3.3085.
        let lowest = |k: usize| samples.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min);
        let highest = |k: usize| {
            samples
                .iter()
                .map(|p| p[k])
                .fold(f64::NEG_INFINITY, f64::max)
        };
        assert!((lowest(1) + 0.4543).abs() < 1e-3, "{}", lowest(1));
        assert!((lowest(2) + 0.8272).abs() < 1e-3, "{}", lowest(2));
        assert!((highest(2) - 3.3085).abs() < 1e-3, "{}", highest(2));
    }

    #[test]
    fn a_rational_spline_keeps_its_weights() {
        // A quarter circle of radius 10 as a rational quadratic: with the
        // middle weight read wrongly it bulges off the circle.
        let model = read(RATIONAL_ARC, Some(1.0)).expect("an IGES model");
        assert_eq!(model.curves.len(), 1);
        let curve = &model.curves[0].0;
        assert_eq!(curve.degree(), 2);
        let (t0, t1) = curve.domain();
        for k in 0..=100 {
            let p = curve.point_at_knot(t0 + (t1 - t0) * k as f64 / 100.0);
            assert!(
                (distance(p, [0.0; 3]) - 10.0).abs() < 1e-9,
                "{p:?} is off the circle"
            );
            assert!(p[2].abs() < 1e-12);
        }
        assert_close(curve.point_at_knot(t0), [10.0, 0.0, 0.0], 1e-12, "start");
        assert_close(curve.point_at_knot(t1), [0.0, 10.0, 0.0], 1e-12, "end");
    }

    #[test]
    fn every_solid_of_a_file_is_read() {
        let model = read(TWO_SOLIDS, Some(1.0)).expect("an IGES model");
        assert_eq!((model.bodies.len(), model.sheets.len()), (2, 0));
        // Compared by position: the order the file lists them in is not
        // what this checks.
        let mut solids: Vec<([f64; 3], [f64; 3], f64)> = model
            .bodies
            .iter()
            .map(|(body, _, _)| {
                let (min, max) = bounds(body);
                (min, max, volume(body))
            })
            .collect();
        solids.sort_by(|a, b| a.0[0].total_cmp(&b.0[0]));
        assert_close(solids[0].0, [0.0, 0.0, 0.0], 1e-6, "box min");
        assert_close(solids[0].1, [10.0, 20.0, 30.0], 1e-6, "box max");
        assert!((solids[0].2 - 6000.0).abs() < 1e-6, "{}", solids[0].2);
        assert_close(solids[1].0, [100.0, 0.0, 0.0], 1e-6, "cube min");
        assert_close(solids[1].1, [105.0, 5.0, 5.0], 1e-6, "cube max");
        assert!((solids[1].2 - 125.0).abs() < 1e-6, "{}", solids[1].2);
    }

    #[test]
    fn colours_levels_and_transforms_reach_the_curves() {
        use document::types::Color;
        let model = read(ATTRIBUTES, Some(1.0)).expect("an IGES model");
        // The fourth curve is used by another entity, so it is not read.
        let read: Vec<(Color, &str, [f64; 3], [f64; 3])> = model
            .curves
            .iter()
            .map(|(curve, color, layer)| {
                let (t0, t1) = curve.domain();
                (
                    *color,
                    layer.as_str(),
                    curve.point_at_knot(t0),
                    curve.point_at_knot(t1),
                )
            })
            .collect();
        assert_eq!(read.len(), 3, "{read:?}");
        assert_eq!(
            (read[0].0, read[0].1),
            (Color::Index(1), "0"),
            "colour 2, red"
        );
        assert_eq!(
            (read[1].0, read[1].1),
            (
                Color::Rgb {
                    r: 153,
                    g: 204,
                    b: 255
                },
                "PIPES"
            ),
            "a colour definition, and a level named by a level property"
        );
        assert_eq!(
            (read[2].0, read[2].1),
            (Color::Index(250), "7"),
            "black, an unnamed level"
        );
        assert_close(read[0].2, [0.0, 0.0, 0.0], 1e-12, "first start");
        assert_close(read[0].3, [10.0, 0.0, 0.0], 1e-12, "first end");
        assert_close(read[1].2, [0.0, 5.0, 0.0], 1e-12, "second start");
        assert_close(read[2].2, [100.0, 10.0, 0.0], 1e-12, "moved by its matrix");
        assert_close(read[2].3, [110.0, 10.0, 0.0], 1e-12, "moved by its matrix");
    }

    #[test]
    fn input_that_is_not_iges_reads_as_nothing() {
        assert!(read(b"", Some(1.0)).is_none());
        assert!(read(b"not an IGES file\n", Some(1.0)).is_none());
        // A directory section with nothing the reader keeps is an empty model.
        let model = read(&SPLINE[..SPLINE.len() / 2], Some(1.0));
        assert!(model.is_none_or(|m| m.bodies.is_empty() && m.sheets.is_empty()));
    }

    /// FreeCAD's solids are sound bodies facing out: each edge is met by
    /// its two faces in opposite senses, also where a face is turned
    /// against its surface in its shell (as the box's and cylinder's are).
    #[test]
    fn solids_are_sound_and_face_out() {
        let mut solids = 0;
        for file in [BOX, CYLINDER, INCH_CUBE, TWO_SOLIDS] {
            for (body, _, _) in &read(file, Some(1.0)).expect("an IGES file").bodies {
                let problems = ocs_solid::model::check(body);
                assert!(problems.is_empty(), "{problems:?}");
                assert!(volume(body) > 0.0);
                solids += 1;
            }
        }
        assert_eq!(solids, 5);
    }

    /// Damaged copies of every fixture: cut at each line, each line left
    /// out, and each number in the parameter data replaced by a value a
    /// careless writer might leave there. None may panic.
    #[test]
    fn damaged_files_never_panic() {
        const VALUES: [&str; 6] = ["0", "-1", "1E308", "-1E308", "NaN", "99999999"];
        let mut cases = 0;
        for bytes in [
            BOX,
            CYLINDER,
            INCH_CUBE,
            SPLINE,
            ATTRIBUTES,
            RATIONAL_ARC,
            TWO_SOLIDS,
        ] {
            let text = std::str::from_utf8(bytes).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            let mut check = |damaged: String| {
                cases += 1;
                let _ = read(damaged.as_bytes(), Some(1.0));
            };
            for n in 0..lines.len() {
                check(lines[..n].join("\n"));
                check([&lines[..n], &lines[n + 1..]].concat().join("\n"));
            }
            let mut value = 0;
            for (n, line) in lines.iter().enumerate() {
                if line.get(72..73) != Some("P") {
                    continue;
                }
                let data = &line[..64];
                let mut start = None;
                for (i, c) in data.char_indices().chain([(64, ',')]) {
                    match (
                        start,
                        c.is_ascii_digit() || matches!(c, '.' | '-' | 'E' | 'D'),
                    ) {
                        (None, true) => start = Some(i),
                        (Some(s), false) => {
                            let replaced = format!(
                                "{:<64}{}",
                                format!(
                                    "{}{}{}",
                                    &data[..s],
                                    VALUES[value % VALUES.len()],
                                    &data[i..]
                                )
                                .trim_end(),
                                &line[64..]
                            );
                            value += 1;
                            let mut damaged = lines.clone();
                            damaged[n] = &replaced;
                            check(damaged.join("\n"));
                            start = None;
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(cases > 1000, "{cases} damaged files");
    }

    /// A trimmed face on a Plane entity (108) comes in as one sheet (#3048).
    #[test]
    fn a_face_on_a_plane_entity_is_read() {
        let model = read(PLANE_108, Some(1.0)).expect("a model");
        assert_eq!(model.sheets.len(), 1);
        let (min, max) = bounds(&model.sheets[0].0);
        assert_close(min, [0.0, 0.0, 0.0], 1e-6, "min");
        assert_close(max, [10.0, 10.0, 0.0], 1e-6, "max");
    }

    /// A revolution of more than one turn is refused instead of growing
    /// without limit (#3047).
    #[test]
    fn a_revolution_of_more_than_one_turn_is_refused() {
        let sheets = read(LARGE_REVOLUTION, Some(1.0)).map_or(0, |m| m.sheets.len());
        assert_eq!(sheets, 0);
    }

}
