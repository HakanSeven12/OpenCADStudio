//! IGES reading: the manifold solids (type 186) of a file, their shells,
//! faces, loops and edges (514/510/508/504/502), its trimmed and bounded
//! surfaces (144/142 and 143/141) and its independent bounded surfaces,
//! over spline surfaces and curves (128/126), parametric spline surfaces
//! (114), ruled surfaces (118), surfaces of revolution (120), tabulated
//! cylinders (122), offsets of planes (140), planes, cylinders, cones,
//! spheres and tori (190–198), lines and arcs (110/100) and composite curves
//! (102), as kernel bodies.

use std::collections::HashMap;

use kernel::brep::{Body, Circle3, Curve3, EdgeKey, Line3, Surface, VertexKey};
use kernel::space::{NurbsCurve3, NurbsSurface3, Plane, Vec3};

use super::brep_build::{Builder, FaceSpec};
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
            let count: usize = chars[start..i].iter().collect::<String>().parse().unwrap_or(0);
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
            let raw: String = chars[start..i].iter().collect::<String>().replace(['D', 'd'], "E");
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
    [[v(1), v(2), v(3), v(4)], [v(5), v(6), v(7), v(8)], [v(9), v(10), v(11), v(12)]]
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
                    (e.params.get(k).map_or(0.0, P::num) * 2.55).round().clamp(0.0, 255.0) as u8
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
        let m = (entity.transform != 0).then(|| self.matrix(entity.transform)).flatten();
        let scale = self.scale;
        move |p| {
            let p = match m {
                Some(m) => [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3]),
                None => p,
            };
            p.map(|c| c * scale)
        }
    }

    fn curve(&self, pointer: f64) -> Option<Curve3> {
        let entity = self.get(pointer)?;
        let p = &entity.params;
        let v = |k: usize| p.get(k).map_or(0.0, P::num);
        let place = self.place(entity);
        Some(match entity.kind {
            110 => {
                let a = place([v(1), v(2), v(3)]);
                let b = place([v(4), v(5), v(6)]);
                Curve3::Line(Line3 {
                    origin: a,
                    direction: [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                })
            }
            100 => {
                let centre = place([v(2), v(3), v(1)]);
                let x = place([v(2) + 1.0, v(3), v(1)]);
                let y = place([v(2), v(3) + 1.0, v(1)]);
                let start = place([v(4), v(5), v(1)]);
                let radius = (0..3).map(|k| (start[k] - centre[k]).powi(2)).sum::<f64>().sqrt();
                Curve3::Circle(Circle3 {
                    plane: Plane::orthonormal(
                        centre,
                        [x[0] - centre[0], x[1] - centre[1], x[2] - centre[2]],
                        {
                            let (a, b) = (
                                [x[0] - centre[0], x[1] - centre[1], x[2] - centre[2]],
                                [y[0] - centre[0], y[1] - centre[1], y[2] - centre[2]],
                            );
                            [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
                        },
                    )?,
                    radius,
                })
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
                Curve3::Nurbs(NurbsCurve3::new(degree, points, knots, Some(weights))?)
            }
            _ => return None,
        })
    }

    /// A boundary curve as its pieces — each a curve and the points it runs
    /// from and to — composite curves taken apart.
    /// Composite curves nest only a few deep, so a deeper (or circular) one
    /// is refused.
    fn pieces(&self, pointer: f64) -> Option<Vec<(Curve3, [f64; 3], [f64; 3])>> {
        self.pieces_at(pointer, 0)
    }

    fn pieces_at(&self, pointer: f64, depth: usize) -> Option<Vec<(Curve3, [f64; 3], [f64; 3])>> {
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
            (Curve3::Nurbs(nurbs), _) => {
                let (t0, t1) = nurbs.domain();
                (nurbs.point_at_knot(t0), nurbs.point_at_knot(t1))
            }
            _ => return None,
        };
        Some(vec![(curve, from, to)])
    }

    /// The model-space pieces of a curve on a surface (142) or a boundary
    /// (141), each of the latter's curves in the sense it is run.
    fn boundary(&self, pointer: f64) -> Option<Vec<(Curve3, [f64; 3], [f64; 3])>> {
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
        Some(match (entity.transform != 0).then(|| self.matrix(entity.transform)).flatten() {
            Some(m) => [0, 1, 2].map(|r| m[r][0] * d[0] + m[r][1] * d[1] + m[r][2] * d[2]),
            None => d,
        })
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
                Curve3::Nurbs(nurbs) => Some(nurbs),
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
    ) -> Option<Surface> {
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
        let frame = |at: Vec3, x: Vec3| {
            Plane::orthonormal(at.to_array(), turned(x, k, start).to_array(), k.to_array())
        };
        Some(if out.length() <= tolerance {
            let x = radial(at(0.5 * (a0 + a1))).normalize()?;
            if [a0, a1].into_iter().any(|a| radial(at(a)).dot(x) < -tolerance) {
                return None;
            }
            Surface::Sphere(kernel::brep::Sphere {
                frame: frame(centre, x)?,
                radius,
            })
        } else {
            let x = out.normalize()?;
            let outermost = centre + x * radius;
            if out.length() <= radius + tolerance || at(a0).distance(outermost) > tolerance {
                return None;
            }
            Surface::Torus(kernel::brep::Torus {
                frame: frame(foot, x)?,
                major_radius: out.length(),
                minor_radius: radius,
            })
        })
    }

    /// The frame of an analytic surface (190–198): its location, its axis
    /// (or normal) and the reference direction of form 1 (else one square
    /// to the axis).
    fn frame(&self, at: f64, axis: Option<f64>, reference: Option<f64>) -> Option<Plane> {
        let origin = self.point_of(at)?;
        let axis = match axis {
            Some(a) => self.direction_of(a)?,
            None => [0.0, 0.0, 1.0],
        };
        let x = match reference.and_then(|r| self.direction_of(r)) {
            Some(x) => x,
            None => super::iges_surfaces::any_perpendicular(kernel::space::Vec3::from(axis)).to_array(),
        };
        Plane::orthonormal(origin, x, axis)
    }

    /// A surface, and the spline whose parameter domain bounds it when the
    /// file trims it with nothing (none for the unbounded analytic ones).
    fn surface_spec(&self, pointer: f64) -> Option<(Surface, Option<NurbsSurface3>)> {
        self.surface_spec_at(pointer, 0)
    }

    /// [`Doc::surface_spec`] with offsets (140) of offsets nested `depth`
    /// deep; a deeper (or circular) chain is refused.
    fn surface_spec_at(
        &self,
        pointer: f64,
        depth: usize,
    ) -> Option<(Surface, Option<NurbsSurface3>)> {
        use super::iges_surfaces as build;
        let entity = self.get(pointer)?;
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        let form1 = |k: usize| (entity.form == 1).then(|| v(k));
        let spline = |s: NurbsSurface3| Some((Surface::Nurbs(s.clone()), Some(s)));
        match entity.kind {
            128 => self.surface(pointer).and_then(|s| match s {
                Surface::Nurbs(n) => spline(n),
                _ => None,
            }),
            190 => Some((Surface::Plane(self.frame(v(1), Some(v(2)), form1(3))?), None)),
            192 => Some((
                Surface::Cylinder(kernel::brep::Cylinder {
                    base: self.frame(v(1), Some(v(2)), form1(4))?,
                    radius: v(3) * self.scale,
                }),
                None,
            )),
            194 => Some((
                Surface::Cone(kernel::brep::Cone {
                    base: self.frame(v(1), Some(v(2)), form1(5))?,
                    radius: v(3) * self.scale,
                    // The file's cone widens along its axis.
                    half_angle: -v(4).to_radians(),
                }),
                None,
            )),
            196 => Some((
                Surface::Sphere(kernel::brep::Sphere {
                    frame: self.frame(v(1), form1(3), form1(4))?,
                    radius: v(2) * self.scale,
                }),
                None,
            )),
            198 => Some((
                Surface::Torus(kernel::brep::Torus {
                    frame: self.frame(v(1), Some(v(2)), form1(5))?,
                    major_radius: v(3) * self.scale,
                    minor_radius: v(4) * self.scale,
                }),
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
                spline(build::tabulate(&directrix, [end[0] - start[0], end[1] - start[1], end[2] - start[2]])?)
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
                let blocks = m.checked_sub(1)?.checked_mul(n.checked_add(1)?)?.checked_add(n)?;
                let end = m.checked_add(n)?.checked_add(7)?.checked_add(blocks.checked_mul(48)?)?;
                if n == 0 || end > entity.params.len() {
                    return None;
                }
                let tu: Vec<f64> = (5..=5 + m).map(v).collect();
                let tv: Vec<f64> = (6 + m..=6 + m + n).map(v).collect();
                let at = 7 + m + n;
                let place = self.place(entity);
                let coefficients: Vec<[[f64; 16]; 3]> = (0..m * n)
                    .map(|k| (k / n) * (n + 1) + k % n)
                    .map(|k| [0, 1, 2].map(|axis| std::array::from_fn(|c| v(at + 48 * k + 16 * axis + c))))
                    .collect();
                let raw = build::patches(&tu, &tv, &coefficients)?;
                // The placement applies to the control points.
                let (du, dv) = raw.degrees();
                let (uk, vk) = raw.knots();
                let rows = raw.control_points().iter().map(|row| row.iter().map(|p| place(*p)).collect()).collect();
                spline(NurbsSurface3::new(du, dv, rows, uk.to_vec(), vk.to_vec(), None)?)
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
                if let Some((Surface::Plane(plane), agrees)) = kernel::brep::analytic_surface(&base) {
                    let n = kernel::space::Vec3::from(plane.normal()?) * (if agrees { 1.0 } else { -1.0 });
                    return spline(build::translated(&base, (n * distance).to_array())?);
                }
                let net = kernel::acis::offset_surface(&base, distance)?;
                // A point of the offset, to read the new radius off.
                let ((u0, u1), (v0, v1)) = net.domain();
                let p = kernel::space::Vec3::from(net.point_at_knot(0.5 * (u0 + u1), 0.5 * (v0 + v1)));
                let around = |frame: &Plane| {
                    let (o, k) = (kernel::space::Vec3::from(frame.origin), kernel::space::Vec3::from(frame.normal()?));
                    let q = p - o;
                    Some((q - k * q.dot(k)).length())
                };
                let offset = match surface {
                    Surface::Cylinder(c) => Surface::Cylinder(kernel::brep::Cylinder { radius: around(&c.base)?, ..c }),
                    Surface::Sphere(s) => Surface::Sphere(kernel::brep::Sphere {
                        radius: (p - kernel::space::Vec3::from(s.frame.origin)).length(),
                        ..s
                    }),
                    Surface::Torus(t) => {
                        let (o, k) = (kernel::space::Vec3::from(t.frame.origin), kernel::space::Vec3::from(t.frame.normal()?));
                        let q = p - o;
                        let radial = q - k * q.dot(k);
                        let tube = o + radial.normalize()? * t.major_radius;
                        Surface::Torus(kernel::brep::Torus { minor_radius: (p - tube).length(), ..t })
                    }
                    // Any other base keeps the offset as such (base and
                    // distance), drawn through its approximation.
                    _ => {
                        return Some((
                            Surface::Offset(kernel::brep::OffsetSurface {
                                base,
                                distance,
                                approximation: net.clone(),
                            }),
                            Some(net),
                        ))
                    }
                };
                Some((offset, Some(net)))
            }
            _ => None,
        }
    }

    /// The model pieces of a curve given in a surface's parameters (x as u,
    /// y as v): straight where the carried points stay on a line, else a
    /// spline through them.
    fn parameter_pieces(&self, curve: f64, surface: f64) -> Option<Vec<(Curve3, [f64; 3], [f64; 3])>> {
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
            let straight = samples
                .iter()
                .all(|p| kernel::space::Vec3::from(*p).distance_to_segment(a.into(), b.into()) <= self.resolution);
            let curve = if straight {
                Curve3::Line(Line3 {
                    origin: a,
                    direction: [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                })
            } else {
                Curve3::Nurbs(NurbsCurve3::interpolate_fit(
                    &samples,
                    None,
                    None,
                    kernel::space::Parameterization::Chord,
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
                Curve3::Nurbs(nurbs) => Some(nurbs.point_at_knot(t)),
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
                Curve3::Nurbs(nurbs) => Some(nurbs.domain()),
                _ => None,
            },
            _ => None,
        }
    }

    /// [`Doc::pieces`] without the drawing scale (parameter-space curves).
    fn pieces_unplaced(&self, pointer: f64) -> Option<Vec<(Curve3, [f64; 3], [f64; 3])>> {
        // ponytail: parameter curves are read through the scaled reader and
        // the scale divided back out; matrices on them are not expected.
        let scale = self.scale;
        Some(
            self.pieces(pointer)?
                .into_iter()
                .map(|(c, a, b)| (unscale(c, scale), a.map(|x| x / scale), b.map(|x| x / scale)))
                .collect(),
        )
    }

    fn surface(&self, pointer: f64) -> Option<Surface> {
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
        NurbsSurface3::new(m1, m2, points, u_knots, v_knots, Some(weights)).map(Surface::Nurbs)
    }
}

struct Assembly<'a> {
    doc: &'a Doc,
    build: Builder,
    vertices: HashMap<(usize, usize), VertexKey>,
    edges: HashMap<(usize, usize), EdgeKey>,
    /// Vertices made where boundary curves meet, by position.
    corners: Vec<([f64; 3], VertexKey)>,
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt()
}

impl Assembly<'_> {
    fn vertex(&mut self, list: f64, index: f64) -> Option<VertexKey> {
        let key = (list.abs() as usize, index as usize);
        if let Some(&v) = self.vertices.get(&key) {
            return Some(v);
        }
        let entity = self.doc.get(list).filter(|e| e.kind == 502)?;
        let n = key.1.checked_sub(1)?.checked_mul(3)?.checked_add(2)?.min(entity.params.len());
        let p = |k: usize| entity.params.get(k).map(P::num);
        let point = self.doc.place(entity)([p(n)?, p(n + 1)?, p(n + 2)?]);
        let v = self.build.vertex(point);
        self.vertices.insert(key, v);
        Some(v)
    }

    fn edge(&mut self, list: f64, index: f64) -> Option<EdgeKey> {
        let key = (list.abs() as usize, index as usize);
        if let Some(&e) = self.edges.get(&key) {
            return Some(e);
        }
        let entity = self.doc.get(list).filter(|e| e.kind == 504)?;
        let n = key.1.checked_sub(1)?.checked_mul(5)?.checked_add(2)?.min(entity.params.len());
        let p = |k: usize| entity.params.get(k).map(P::num);
        let curve = self.doc.curve(p(n)?)?;
        let start = self.vertex(p(n + 1)?, p(n + 2)?)?;
        let end = self.vertex(p(n + 3)?, p(n + 4)?)?;
        let e = self.build.edge(curve, start, end)?;
        self.edges.insert(key, e);
        Some(e)
    }

    /// The vertex at a point, shared with one already there.
    fn corner(&mut self, point: [f64; 3]) -> VertexKey {
        let tolerance = self.doc.resolution;
        if let Some((_, v)) = self.corners.iter().find(|(p, _)| distance(*p, point) <= tolerance) {
            return *v;
        }
        let v = self.build.vertex(point);
        self.corners.push((point, v));
        v
    }

    /// A loop along curve pieces, in their order.
    fn curve_ring(&mut self, pieces: Vec<(Curve3, [f64; 3], [f64; 3])>) -> Option<Vec<(EdgeKey, bool)>> {
        // Pieces of one loop follow on from each other: a piece starts where
        // the one before it ends (and the last runs back to the first) even
        // when the file's points part by more than its resolution, up to a
        // ten-thousandth of the loop's size.
        let (lo, hi) = pieces.iter().flat_map(|(_, f, t)| [*f, *t]).fold(
            ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
            |(lo, hi), p| ([0, 1, 2].map(|k| lo[k].min(p[k])), [0, 1, 2].map(|k| hi[k].max(p[k]))),
        );
        let gap = (1e-4 * distance(lo, hi)).max(self.doc.resolution);
        let mut ring = Vec::new();
        let mut first: Option<([f64; 3], VertexKey)> = None;
        let mut last: Option<([f64; 3], VertexKey)> = None;
        let count = pieces.len();
        for (k, (curve, from, to)) in pieces.into_iter().enumerate() {
            if distance(from, to) <= self.doc.resolution && matches!(curve, Curve3::Line(_)) {
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
            ring.push((self.build.edge(curve, a, b)?, true));
        }
        (!ring.is_empty()).then_some(ring)
    }

    /// A loop round the edge of a spline surface's parameter domain; sides
    /// that close up to a point (a pole) are left out, and a side that runs
    /// back along the opposite one (the surface closes up there, a seam) is
    /// that side's edge used the other way. Also whether every side closed
    /// up so (the sheet bounds a volume).
    fn domain_ring(&mut self, surface: &NurbsSurface3) -> Option<(Vec<(EdgeKey, bool)>, bool)> {
        let ((u0, u1), (v0, v1)) = surface.domain();
        let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
        let sides = [(1, v0), (0, u1), (1, v1), (0, u0)];
        let tolerance = self.doc.resolution;
        let mut traced: Vec<Option<([f64; 3], [f64; 3], [f64; 3], EdgeKey)>> = Vec::new();
        let mut ring = Vec::new();
        for (k, (axis, at)) in sides.into_iter().enumerate() {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let (from, to) = (surface.point_at_knot(a.0, a.1), surface.point_at_knot(b.0, b.1));
            let middle = surface.point_at_knot(0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1));
            if distance(from, middle) <= tolerance && distance(to, middle) <= tolerance {
                traced.push(None);
                continue;
            }
            let seam = (k >= 2).then(|| traced[k - 2]).flatten().filter(|(f, t, m, _)| {
                distance(*f, to) <= tolerance && distance(*t, from) <= tolerance && distance(*m, middle) <= tolerance
            });
            if let Some((.., edge)) = seam {
                ring.push((edge, false));
                traced[k - 2] = None;
                traced.push(None);
                continue;
            }
            let (p, q) = (self.corner(from), self.corner(to));
            let edge = self.build.edge(Curve3::Nurbs(surface.isocurve(axis, at)?), p, q)?;
            ring.push((edge, true));
            traced.push(Some((from, to, middle, edge)));
        }
        // Closed when no side is left without its seam partner.
        let closed = traced.iter().all(Option::is_none);
        (!ring.is_empty()).then_some((ring, closed))
    }

    /// Turns a loop to run counter-clockwise round the surface's normal (an
    /// outer one) or clockwise (a hole), judged by the area it encloses in
    /// the surface's parameters.
    fn orient(&self, surface: &Surface, ring: &mut [(EdgeKey, bool)], outer: bool) {
        let mut uv = Vec::new();
        for (edge, forward) in ring.iter() {
            let Some(edge) = self.build.body.edges.get(*edge) else { return };
            let Some(curve) = self.build.body.curves.get(edge.curve) else { return };
            let (t0, t1) = (edge.start_parameter, edge.end_parameter);
            for k in 0..8 {
                let s = k as f64 / 8.0;
                let s = if *forward { s } else { 1.0 - s };
                if let Some(p) = surface.parameters_at(curve.point_at(t0 + (t1 - t0) * s)) {
                    uv.push(p);
                }
            }
        }
        let area: f64 = (0..uv.len())
            .map(|k| {
                let (a, b) = (uv[k], uv[(k + 1) % uv.len()]);
                a.0 * b.1 - b.0 * a.1
            })
            .sum();
        if (area > 0.0) != outer {
            ring.reverse();
            for (_, forward) in ring.iter_mut() {
                *forward = !*forward;
            }
        }
    }

    /// A trimmed (144) or bounded (143) surface as a one-face sheet.
    fn sheet(&mut self, entity: &Entity) -> Option<()> {
        let v = |k: usize| entity.params.get(k).map_or(0.0, P::num);
        // A count can be no more than the parameters there are.
        let count = (v(3) as usize).min(entity.params.len());
        let (surface_at, outer, holes): (f64, Option<f64>, Vec<f64>) = match entity.kind {
            144 => (v(1), (v(2) != 0.0).then(|| v(4)), (0..count).map(|k| v(5 + k)).collect()),
            143 => (v(2), (count > 0).then(|| v(4)), (1..count).map(|k| v(4 + k)).collect()),
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
        self.orient(&surface, &mut first, true);
        let mut loops = vec![first];
        for hole in holes {
            let pieces = self.doc.boundary(hole)?;
            let mut ring = self.curve_ring(pieces)?;
            self.orient(&surface, &mut ring, false);
            loops.push(ring);
        }
        self.build.lump(vec![vec![FaceSpec {
            surface,
            forward: true,
            loops,
        }]])
    }

    fn ring(&mut self, pointer: f64) -> Option<Vec<(EdgeKey, bool)>> {
        let entity = self.doc.get(pointer).filter(|e| e.kind == 508)?;
        let p = |k: usize| entity.params.get(k).map(P::num);
        let mut ring = Vec::new();
        let mut at = 2;
        for _ in 0..p(1)? as usize {
            let (kind, list, index, agrees, pcurves) = (p(at)?, p(at + 1)?, p(at + 2)?, p(at + 3)?, p(at + 4)?);
            if kind == 0.0 {
                ring.push((self.edge(list, index)?, agrees != 0.0));
            }
            at = at.saturating_add(5).saturating_add((pcurves as usize).saturating_mul(2));
        }
        Some(ring)
    }

    /// An independent bounded surface as a one-face sheet over its whole
    /// parameter domain; `true` when the face closes up on itself all round
    /// (a whole sphere or torus) into a sound solid, turned to face out.
    fn whole(&mut self, pointer: f64) -> Option<bool> {
        let (surface, net) = self.doc.surface_spec(pointer)?;
        let (mut ring, closed) = self.domain_ring(net.as_ref()?)?;
        self.orient(&surface, &mut ring, true);
        self.build.lump(vec![vec![FaceSpec {
            surface,
            forward: true,
            loops: vec![ring],
        }]])?;
        let body = &mut self.build.body;
        let closed = closed && body.validate().is_empty();
        if closed && inside_out(body) {
            for face in body.faces.values_mut() {
                face.forward = !face.forward;
            }
            for ring in body.loops.values_mut() {
                ring.coedges.reverse();
            }
            for coedge in body.coedges.values_mut() {
                coedge.forward = !coedge.forward;
            }
        }
        Some(closed)
    }

    fn face(&mut self, pointer: f64, agrees: bool) -> Option<FaceSpec> {
        let entity = self.doc.get(pointer).filter(|e| e.kind == 510)?;
        let p = |k: usize| entity.params.get(k).map(P::num);
        let surface = self.doc.surface_spec(p(1)?)?.0;
        let loops = (0..p(2)? as usize)
            .map(|n| self.ring(p(4 + n)?))
            .collect::<Option<Vec<_>>>()?;
        Some(FaceSpec {
            surface,
            forward: agrees,
            loops,
        })
    }

    fn shell(&mut self, pointer: f64, agrees: bool) -> Option<Vec<FaceSpec>> {
        let entity = self.doc.get(pointer).filter(|e| e.kind == 514)?;
        let p = |k: usize| entity.params.get(k).map(P::num);
        (0..p(1)? as usize)
            .map(|n| {
                let face_agrees = p(3 + 2 * n)? != 0.0;
                let mut face = self.face(p(2 + 2 * n)?, face_agrees == agrees)?;
                if !agrees {
                    for ring in &mut face.loops {
                        ring.reverse();
                        for (_, f) in ring.iter_mut() {
                            *f = !*f;
                        }
                    }
                }
                Some(face)
            })
            .collect()
    }
}

/// What an IGES file holds.
pub struct IgesModel {
    /// Each solid, scaled to the drawing, with its colour and layer.
    pub bodies: Vec<(kernel::brep::Body, document::types::Color, String)>,
    /// Each trimmed or bounded surface, a one-face sheet, with its colour
    /// and layer.
    pub sheets: Vec<(kernel::brep::Body, document::types::Color, String)>,
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
        let Some(section) = line.get(72..73).and_then(|s| s.chars().next()) else { continue };
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
            Some(P::Str(name)) if name.eq_ignore_ascii_case("IN") || name.eq_ignore_ascii_case("INCH") => 25.4,
            Some(P::Str(name)) if name.eq_ignore_ascii_case("FT") => 304.8,
            Some(P::Str(name)) if name.eq_ignore_ascii_case("M") => 1000.0,
            Some(P::Str(name)) if name.eq_ignore_ascii_case("CM") => 10.0,
            _ => 1.0,
        },
        _ => 1.0,
    } * globals.get(12).map(P::num).filter(|s| *s > 0.0).map_or(1.0, |s| 1.0 / s);

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
        n if n > 0 => level_names.get(&n).cloned().unwrap_or_else(|| n.to_string()),
        _ => "0".to_string(),
    };
    let scale = drawing_mm.filter(|mm| *mm > 0.0).map_or(1.0, |mm| unit_mm / mm);
    let resolution = globals.get(18).map(P::num).filter(|r| *r > 0.0).unwrap_or(1e-6) * scale;
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
        let Some((shell, agrees)) = v(1).zip(v(2)) else { continue };
        let mut assembly = Assembly {
            doc: &doc,
            build: Builder {
                closed_only: true,
                ..Builder::new()
            },
            vertices: HashMap::new(),
            edges: HashMap::new(),
            corners: Vec::new(),
        };
        let mut shells = Vec::new();
        let outer = assembly.shell(shell, agrees != 0.0);
        let voids = (0..v(3).unwrap_or(0.0) as usize)
            .map(|n| assembly.shell(v(4 + 2 * n)?, v(5 + 2 * n)? != 0.0))
            .collect::<Option<Vec<_>>>();
        if let (Some(outer), Some(voids)) = (outer, voids) {
            shells.push(outer);
            shells.extend(voids);
            if assembly.build.lump(shells).is_some() {
                bodies.push((assembly.build.body, doc.color(entity.color), layer(entity)));
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
        let mut assembly = Assembly {
            doc: &doc,
            build: Builder {
                closed_only: true,
                ..Builder::new()
            },
            vertices: HashMap::new(),
            edges: HashMap::new(),
            corners: Vec::new(),
        };
        let made = match entity.kind {
            143 | 144 => assembly.sheet(entity).map(|_| false),
            _ => assembly.whole(number as f64),
        };
        match made {
            Some(true) => {
                bodies.push((assembly.build.body, doc.color(entity.color), layer(entity)))
            }
            Some(false) => {
                sheets.push((assembly.build.body, doc.color(entity.color), layer(entity)))
            }
            None => {}
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
            Some((doc.nurbs_curve(number as f64)?, doc.color(entity.color), layer(entity)))
        })
        .collect();
    Some(IgesModel { bodies, sheets, curves })
}

/// A point along a parameter-space curve piece at `t` in 0..=1; an arc
/// runs counter-clockwise from the piece's start to its end.
fn piece_point((curve, from, to): &(Curve3, [f64; 3], [f64; 3]), t: f64) -> Option<[f64; 3]> {
    Some(match curve {
        Curve3::Line(l) => [0, 1, 2].map(|k| l.origin[k] + l.direction[k] * t),
        Curve3::Nurbs(n) => {
            let (a, b) = n.domain();
            n.point_at_knot(a + (b - a) * t)
        }
        Curve3::Circle(c) => {
            let angle = |p: [f64; 3]| c.plane.project(p).map(|[x, y]| y.atan2(x));
            let (a0, mut a1) = (angle(*from)?, angle(*to)?);
            while a1 <= a0 + 1e-12 {
                a1 += std::f64::consts::TAU;
            }
            curve.point_at(a0 + (a1 - a0) * t)
        }
        _ => return None,
    })
}

/// A curve read with the drawing scale, back in file units.
fn unscale(curve: Curve3, scale: f64) -> Curve3 {
    match curve {
        Curve3::Line(l) => Curve3::Line(Line3 {
            origin: l.origin.map(|c| c / scale),
            direction: l.direction.map(|c| c / scale),
        }),
        Curve3::Nurbs(n) => {
            let points = n.control_points().iter().map(|p| p.map(|c| c / scale)).collect();
            NurbsCurve3::new(n.degree(), points, n.knots().to_vec(), Some(n.weights().to_vec()))
                .map(Curve3::Nurbs)
                .unwrap_or(Curve3::Nurbs(n))
        }
        Curve3::Circle(mut c) => {
            c.plane.origin = c.plane.origin.map(|x| x / scale);
            c.radius /= scale;
            Curve3::Circle(c)
        }
        other => other,
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
) -> Option<Surface> {
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
    let base = Plane::orthonormal(foot(far).to_array(), x_axis, k.to_array())?;
    Some(if (r_far - r_near).abs() <= tolerance {
        Surface::Cylinder(kernel::brep::Cylinder { base, radius: r_far })
    } else {
        Surface::Cone(kernel::brep::Cone {
            base,
            radius: r_far,
            half_angle: ((r_far - r_near) / along).atan(),
        })
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

/// Whether a closed body's faces point inward: the volume its mesh encloses
/// comes out negative.
fn inside_out(body: &Body) -> bool {
    let Some(bounds) = kernel::brep::body_bounds(body) else { return false };
    let size = Vec3::from(bounds.min).distance(Vec3::from(bounds.max));
    let mesh = kernel::brep::mesh_body(body, 0.5, size * 1e-2);
    let volume: f64 = mesh
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| Vec3::from(mesh.positions[i]));
            a.dot(b.cross(c))
        })
        .sum();
    volume < 0.0
}
