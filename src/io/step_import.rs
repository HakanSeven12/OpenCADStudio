//! STEP (ISO 10303-21) reading: the B-rep solids and shell models of an
//! AP203/AP214/AP242 file, as kernel bodies, with the product's name and the
//! file's length unit.

use std::collections::{HashMap, HashSet};

use kernel::brep::{
    Circle3, Cone, Curve3, Cylinder, EdgeKey, Ellipse3, Line3, Sphere, Surface, Torus, VertexKey,
};
use kernel::space::{NurbsCurve3, NurbsSurface3, Plane, Vec3};

use super::brep_build::{Builder, FaceSpec};

/// One parameter of an entity instance.
#[derive(Debug, Clone)]
enum P {
    Num(f64),
    Str(String),
    Enum(String),
    Ref(u64),
    List(Vec<P>),
    Null,
    /// A typed value, such as `LENGTH_MEASURE(1.)`.
    Typed(Vec<P>),
}

impl P {
    fn num(&self) -> Option<f64> {
        match self {
            P::Num(v) => Some(*v),
            P::Typed(inner) => inner.first()?.num(),
            _ => None,
        }
    }
    fn reference(&self) -> Option<u64> {
        match self {
            P::Ref(id) => Some(*id),
            _ => None,
        }
    }
    fn list(&self) -> &[P] {
        match self {
            P::List(items) => items,
            _ => &[],
        }
    }
    fn truth(&self) -> bool {
        matches!(self, P::Enum(e) if e == "T")
    }
    fn text(&self) -> Option<&str> {
        match self {
            P::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// An instance: one `(type, parameters)` part, or several for a complex one.
type Instance = Vec<(String, Vec<P>)>;

struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
    /// How many parameter lists deep the cursor is.
    depth: usize,
    /// Scratch shared by every number token of the file: a `.` that needs
    /// a `0` after it is normalized here instead of allocating a fresh
    /// String per number.
    buf: Vec<u8>,
}

impl Cursor<'_> {
    fn skip(&mut self) {
        loop {
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
                self.i += 1;
            }
            if self
                .s
                .get(self.i..)
                .is_some_and(|rest| rest.starts_with(b"/*"))
            {
                match self.s[self.i + 2..].windows(2).position(|w| w == b"*/") {
                    Some(end) => self.i += end + 4,
                    None => self.i = self.s.len(),
                }
            } else {
                break;
            }
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: u8) -> Option<()> {
        (self.peek()? == c).then(|| self.i += 1)
    }
    /// The bytes of the next word, without materializing them: they run to
    /// the first byte that is not alphanumeric, `_` or `-`.
    fn word_span(&mut self) -> std::ops::Range<usize> {
        self.skip();
        let start = self.i;
        while self
            .s
            .get(self.i)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
        {
            self.i += 1;
        }
        start..self.i
    }
    /// The next word, uppercased for keyword matching — one allocation,
    /// only for the words that are actually stored (entity names, enum
    /// values). Callers that just parse or check the word use
    /// [`Cursor::word_span`] / [`Cursor::word_id`] and allocate nothing.
    fn word(&mut self) -> String {
        let span = self.word_span();
        String::from_utf8_lossy(&self.s[span]).to_ascii_uppercase()
    }
    /// The next word read as an instance id (`#12`, `#345`). Digits are
    /// untouched by the uppercasing [`Cursor::word`] would apply, so the
    /// copy could never change the parse.
    fn word_id(&mut self) -> Option<u64> {
        let span = self.word_span();
        std::str::from_utf8(&self.s[span]).ok()?.parse().ok()
    }
    /// Whether Rust's float parser needs a `0` written after the `.` at
    /// `i`: it takes `1.E-3` as nothing (it wants `1.0E-3`), while a dot
    /// with no digit anywhere before it — the malformed `+.` — is a
    /// rejection the reader has always passed straight through.
    fn dot_needs_zero(raw: &[u8], i: usize) -> bool {
        let next = raw.get(i + 1);
        !next.is_some_and(|n| n.is_ascii_digit())
            && (matches!(next, Some(b'e' | b'E')) || raw[..i].iter().any(u8::is_ascii_digit))
    }
    /// One number token, parsed in place: a `.` that needs a `0` after it
    /// (per [`Cursor::dot_needs_zero`]) is normalized into the scratch
    /// `buf` this cursor shares across every token of the file, and
    /// tokens needing no `0` parse straight out of the file's bytes —
    /// where two `str::replace` calls used to allocate two Strings for
    /// every number in every file.
    fn number(&mut self, start: usize, end: usize) -> Option<f64> {
        let raw = &self.s[start..end];
        if !raw
            .iter()
            .enumerate()
            .any(|(i, &c)| c == b'.' && Self::dot_needs_zero(raw, i))
        {
            return std::str::from_utf8(raw).ok()?.parse().ok();
        }
        self.buf.clear();
        for (i, &c) in raw.iter().enumerate() {
            self.buf.push(c);
            if c == b'.' && Self::dot_needs_zero(raw, i) {
                self.buf.push(b'0');
            }
        }
        std::str::from_utf8(&self.buf).ok()?.parse().ok()
    }
    fn param(&mut self) -> Option<P> {
        Some(match self.peek()? {
            b'#' => {
                self.i += 1;
                P::Ref(self.word_id()?)
            }
            b'\'' => {
                self.i += 1;
                let mut text = Vec::new();
                loop {
                    let c = *self.s.get(self.i)?;
                    self.i += 1;
                    if c == b'\'' {
                        if self.s.get(self.i) == Some(&b'\'') {
                            self.i += 1;
                        } else {
                            break;
                        }
                    }
                    text.push(c);
                }
                P::Str(String::from_utf8_lossy(&text).into_owned())
            }
            b'.' => {
                self.i += 1;
                let value = self.word();
                self.eat(b'.')?;
                P::Enum(value)
            }
            b'$' | b'*' => {
                self.i += 1;
                P::Null
            }
            b'(' => P::List(self.params()?),
            c if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' => {
                let start = self.i;
                while self
                    .s
                    .get(self.i)
                    .is_some_and(|c| c.is_ascii_digit() || b"+-.eE".contains(c))
                {
                    self.i += 1;
                }
                P::Num(self.number(start, self.i)?)
            }
            _ => {
                if self.word_span().is_empty() {
                    return None;
                }
                P::Typed(self.params()?)
            }
        })
    }
    /// A parameter list; lists nested past any real file's depth are
    /// refused rather than read on until the stack runs out.
    fn params(&mut self) -> Option<Vec<P>> {
        if self.depth >= 64 {
            return None;
        }
        self.depth += 1;
        let items = self.list();
        self.depth -= 1;
        items
    }
    fn list(&mut self) -> Option<Vec<P>> {
        self.eat(b'(')?;
        let mut items = Vec::new();
        if self.eat(b')').is_some() {
            return Some(items);
        }
        loop {
            items.push(self.param()?);
            match self.peek()? {
                b',' => self.i += 1,
                b')' => {
                    self.i += 1;
                    return Some(items);
                }
                _ => return None,
            }
        }
    }
    /// `#id = TYPE(...)` or `#id = (A(...) B(...))`, up to its `;`.
    fn instance(&mut self) -> Option<(u64, Instance)> {
        self.eat(b'#')?;
        let id = self.word_id()?;
        self.eat(b'=')?;
        let mut parts = Vec::new();
        if self.eat(b'(').is_some() {
            while self.eat(b')').is_none() {
                let name = self.word();
                if name.is_empty() {
                    return None;
                }
                parts.push((name, self.params()?));
            }
        } else {
            let name = self.word();
            parts.push((name, self.params()?));
        }
        self.eat(b';')?;
        Some((id, parts))
    }
}

struct Doc {
    instances: HashMap<u64, Instance>,
    /// Plane-angle unit in radians.
    angle: f64,
    /// The colour each styled item (a solid, a shell or a face) is shown
    /// in, as 0–1 RGB.
    styles: HashMap<u64, [f64; 3]>,
}

/// An RGB colour as 0–1 values.
pub type Rgb = [f64; 3];

impl Doc {
    /// A presentation colour: an RGB one or a predefined one.
    fn colour(&self, p: &P) -> Option<Rgb> {
        let (kind, params) = self.simple(p.reference()?)?;
        match kind {
            "COLOUR_RGB" => Some([
                params.get(1)?.num()?,
                params.get(2)?.num()?,
                params.get(3)?.num()?,
            ]),
            "DRAUGHTING_PRE_DEFINED_COLOUR" => Some(match params.first()?.text()? {
                "red" => [1.0, 0.0, 0.0],
                "green" => [0.0, 1.0, 0.0],
                "blue" => [0.0, 0.0, 1.0],
                "yellow" => [1.0, 1.0, 0.0],
                "magenta" => [1.0, 0.0, 1.0],
                "cyan" => [0.0, 1.0, 1.0],
                "black" => [0.0, 0.0, 0.0],
                "white" => [1.0, 1.0, 1.0],
                _ => return None,
            }),
            _ => None,
        }
    }

    /// The colour a styled item's presentation styles give it: its surface
    /// fill (or rendering) colour, or failing that its curve colour.
    fn style_colour(&self, styles: &P) -> Option<Rgb> {
        let mut stack: Vec<&P> = vec![styles];
        let mut seen = Vec::new();
        let mut curve = None;
        while let Some(p) = stack.pop() {
            match p {
                P::List(items) | P::Typed(items) => stack.extend(items.iter().rev()),
                P::Ref(id) if !seen.contains(id) => {
                    seen.push(*id);
                    let Some((kind, params)) = self.simple(*id) else {
                        continue;
                    };
                    match kind {
                        "FILL_AREA_STYLE_COLOUR" => {
                            if let Some(c) = params.get(1).and_then(|c| self.colour(c)) {
                                return Some(c);
                            }
                        }
                        "SURFACE_STYLE_RENDERING" | "SURFACE_STYLE_RENDERING_WITH_PROPERTIES" => {
                            if let Some(c) = params.get(1).and_then(|c| self.colour(c)) {
                                return Some(c);
                            }
                        }
                        "CURVE_STYLE" => {
                            curve = curve.or_else(|| params.get(3).and_then(|c| self.colour(c)))
                        }
                        _ => stack.extend(params.iter().rev()),
                    }
                }
                _ => {}
            }
        }
        curve
    }

    fn parts(&self, id: u64) -> Option<&Instance> {
        self.instances.get(&id)
    }
    fn kind(&self, id: u64) -> &str {
        self.parts(id)
            .and_then(|parts| parts.first())
            .map_or("", |(name, _)| name.as_str())
    }
    /// The parameters of the part named `name`.
    fn part(&self, id: u64, name: &str) -> Option<&[P]> {
        self.parts(id)?
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.as_slice())
    }
    /// A simple instance's type and parameters.
    fn simple(&self, id: u64) -> Option<(&str, &[P])> {
        let parts = self.parts(id)?;
        let (name, params) = parts.first()?;
        Some((name.as_str(), params.as_slice()))
    }

    fn point(&self, p: &P) -> Option<[f64; 3]> {
        let (_, params) = self.simple(p.reference()?)?;
        let c = params.get(1)?.list();
        Some([
            c.first()?.num()?,
            c.get(1)?.num()?,
            c.get(2).and_then(P::num).unwrap_or(0.0),
        ])
    }
    fn direction(&self, p: &P) -> Option<[f64; 3]> {
        self.point(p)
    }
    fn placement(&self, p: &P) -> Option<Plane> {
        let (_, params) = self.simple(p.reference()?)?;
        let origin = self.point(params.get(1)?)?;
        let z = params
            .get(2)
            .and_then(|d| self.direction(d))
            .unwrap_or([0.0, 0.0, 1.0]);
        let x = params
            .get(3)
            .and_then(|d| self.direction(d))
            .filter(|x| Vec3::from(*x).cross(Vec3::from(z)).length() > 1e-9)
            // Any axis off the normal; the plane squares it up.
            .unwrap_or(if z[0].abs() < 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            });
        Plane::orthonormal(origin, x, z)
    }

    /// Knots expanded by their multiplicities. No knot repeats more than
    /// `degree + 1` times, so a bogus multiplicity cannot blow up the list.
    fn knots(mults: &P, knots: &P, degree: usize) -> Option<Vec<f64>> {
        let mut out = Vec::new();
        for (m, k) in mults.list().iter().zip(knots.list()) {
            for _ in 0..(m.num()? as usize).min(degree.saturating_add(1)) {
                out.push(k.num()?);
            }
        }
        Some(out)
    }

    fn curve(&self, p: &P) -> Option<Curve3> {
        let mut id = p.reference()?;
        // Curves that wrap a basis curve are followed to it; the hop limit
        // stops curves that wrap each other in a loop.
        let mut hops = 0;
        while matches!(
            self.kind(id),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" | "TRIMMED_CURVE"
        ) {
            hops += 1;
            if hops > 32 {
                return None;
            }
            id = self.simple(id)?.1.get(1)?.reference()?;
        }
        let kind = self.kind(id);
        if kind == "B_SPLINE_CURVE_WITH_KNOTS" || self.part(id, "B_SPLINE_CURVE").is_some() {
            return self.spline_curve(id).map(Curve3::Nurbs);
        }
        let (_, params) = self.simple(id)?;
        Some(match kind {
            "LINE" => {
                let origin = self.point(params.get(1)?)?;
                let (_, vector) = self.simple(params.get(2)?.reference()?)?;
                let direction = Vec3::from(self.direction(vector.get(1)?)?).normalize()?
                    * vector.get(2)?.num()?;
                Curve3::Line(Line3 {
                    origin,
                    direction: direction.to_array(),
                })
            }
            "CIRCLE" => Curve3::Circle(Circle3 {
                plane: self.placement(params.get(1)?)?,
                radius: params.get(2)?.num()?,
            }),
            "ELLIPSE" => Curve3::Ellipse(Ellipse3 {
                plane: self.placement(params.get(1)?)?,
                major_radius: params.get(2)?.num()?,
                minor_radius: params.get(3)?.num()?,
            }),
            "POLYLINE" => {
                let points: Option<Vec<_>> = params
                    .get(1)?
                    .list()
                    .iter()
                    .map(|q| self.point(q))
                    .collect();
                Curve3::Nurbs(NurbsCurve3::new(1, points?, Vec::new(), None)?)
            }
            _ => return None,
        })
    }

    fn spline_curve(&self, id: u64) -> Option<NurbsCurve3> {
        // The simple form carries everything after its name; the complex
        // form splits it over parts that have no name.
        let (degree, points, mults, knots, weights) = if let Some(p) = self
            .part(id, "B_SPLINE_CURVE_WITH_KNOTS")
            .filter(|p| p.len() >= 9)
        {
            (&p[1], &p[2], &p[6], &p[7], None)
        } else {
            let base = self.part(id, "B_SPLINE_CURVE")?;
            let with = self.part(id, "B_SPLINE_CURVE_WITH_KNOTS")?;
            let weights = self
                .part(id, "RATIONAL_B_SPLINE_CURVE")
                .and_then(|w| w.first());
            (
                base.first()?,
                base.get(1)?,
                with.first()?,
                with.get(1)?,
                weights,
            )
        };
        let points: Option<Vec<_>> = points.list().iter().map(|q| self.point(q)).collect();
        let weights: Option<Vec<f64>> =
            weights.map(|w| w.list().iter().filter_map(P::num).collect());
        let degree = degree.num()? as usize;
        let points = points?;
        let knots = Self::knots(mults, knots, degree)?;
        match &weights {
            Some(w) => NurbsCurve3::new_strict(degree, points.clone(), knots.clone(), w.clone())
                .or_else(|| NurbsCurve3::new(degree, points, knots, weights.clone())),
            None => NurbsCurve3::new(degree, points, knots, None),
        }
    }

    fn surface(&self, p: &P) -> Option<Surface> {
        let id = p.reference()?;
        let kind = self.kind(id);
        if kind == "B_SPLINE_SURFACE_WITH_KNOTS" || self.part(id, "B_SPLINE_SURFACE").is_some() {
            return self.spline_surface(id).map(Surface::Nurbs);
        }
        let (_, params) = self.simple(id)?;
        let frame = self.placement(params.get(1)?)?;
        let value = |k: usize| params.get(k).and_then(P::num);
        Some(match kind {
            "PLANE" => Surface::Plane(frame),
            "CYLINDRICAL_SURFACE" => Surface::Cylinder(Cylinder {
                base: frame,
                radius: value(2)?,
            }),
            // A STEP cone widens along its axis by its semi-angle.
            "CONICAL_SURFACE" => Surface::Cone(Cone {
                base: frame,
                radius: value(2)?,
                half_angle: -value(3)? * self.angle,
            }),
            "SPHERICAL_SURFACE" => Surface::Sphere(Sphere {
                frame,
                radius: value(2)?,
            }),
            "TOROIDAL_SURFACE" => Surface::Torus(Torus {
                frame,
                major_radius: value(2)?,
                minor_radius: value(3)?,
            }),
            _ => return None,
        })
    }

    fn spline_surface(&self, id: u64) -> Option<NurbsSurface3> {
        let (ud, vd, points, um, vm, uk, vk, weights) = if let Some(p) = self
            .part(id, "B_SPLINE_SURFACE_WITH_KNOTS")
            .filter(|p| p.len() >= 13)
        {
            (&p[1], &p[2], &p[3], &p[8], &p[9], &p[10], &p[11], None)
        } else {
            let base = self.part(id, "B_SPLINE_SURFACE")?;
            let with = self.part(id, "B_SPLINE_SURFACE_WITH_KNOTS")?;
            let weights = self
                .part(id, "RATIONAL_B_SPLINE_SURFACE")
                .and_then(|w| w.first());
            (
                base.first()?,
                base.get(1)?,
                base.get(2)?,
                with.first()?,
                with.get(1)?,
                with.get(2)?,
                with.get(3)?,
                weights,
            )
        };
        let rows: Option<Vec<Vec<[f64; 3]>>> = points
            .list()
            .iter()
            .map(|row| row.list().iter().map(|q| self.point(q)).collect())
            .collect();
        let weights: Option<Vec<Vec<f64>>> = weights.map(|w| {
            w.list()
                .iter()
                .map(|row| row.list().iter().filter_map(P::num).collect())
                .collect()
        });
        let (ud, vd) = (ud.num()? as usize, vd.num()? as usize);
        NurbsSurface3::new(
            ud,
            vd,
            rows?,
            Self::knots(um, uk, ud)?,
            Self::knots(vm, vk, vd)?,
            weights,
        )
    }
}

/// Topology of one body, keyed by the file's instance ids.
struct Assembly<'a> {
    doc: &'a Doc,
    build: Builder,
    vertices: HashMap<u64, VertexKey>,
    /// Each edge, and whether it runs from the file's edge end to its start.
    edges: HashMap<u64, (EdgeKey, bool)>,
    /// The file's face of each face built, in build order.
    faces: Vec<u64>,
}

impl Assembly<'_> {
    fn vertex(&mut self, p: &P) -> Option<VertexKey> {
        let id = p.reference()?;
        if let Some(&v) = self.vertices.get(&id) {
            return Some(v);
        }
        let (_, params) = self.doc.simple(id)?;
        let v = self.build.vertex(self.doc.point(params.get(1)?)?);
        self.vertices.insert(id, v);
        Some(v)
    }

    fn edge(&mut self, p: &P) -> Option<(EdgeKey, bool)> {
        let id = p.reference()?;
        if let Some(&e) = self.edges.get(&id) {
            return Some(e);
        }
        let (_, params) = self.doc.simple(id)?;
        let (a, b) = (self.vertex(params.get(1)?)?, self.vertex(params.get(2)?)?);
        let curve = self.doc.curve(params.get(3)?)?;
        let same_sense = params.get(4).is_none_or(P::truth);
        let key = if same_sense {
            self.build.edge(curve, a, b)?
        } else {
            self.build.edge(curve, b, a)?
        };
        self.edges.insert(id, (key, !same_sense));
        Some((key, !same_sense))
    }

    /// A bound's coedges, in the order the face runs them.
    fn bound(&mut self, p: &P) -> Option<Vec<(EdgeKey, bool)>> {
        let (_, params) = self.doc.simple(p.reference()?)?;
        let ring_id = params.get(1)?.reference()?;
        let agrees = params.get(2).is_none_or(P::truth);
        let mut ring = Vec::new();
        if self.doc.kind(ring_id) == "EDGE_LOOP" {
            let (_, loop_params) = self.doc.simple(ring_id)?;
            for oriented in loop_params.get(1)?.list() {
                let (_, o) = self.doc.simple(oriented.reference()?)?;
                let (edge, flipped) = self.edge(o.get(3)?)?;
                let along = o.get(4).is_none_or(P::truth);
                ring.push((edge, along != flipped));
            }
        }
        if !agrees {
            ring.reverse();
            for (_, forward) in &mut ring {
                *forward = !*forward;
            }
        }
        Some(ring)
    }

    fn face(&mut self, p: &P, flip: bool) -> Option<FaceSpec> {
        let id = p.reference()?;
        self.faces.push(id);
        let (_, params) = self.doc.simple(id)?;
        let surface = self.doc.surface(params.get(2)?)?;
        let mut outer = Vec::new();
        let mut inner = Vec::new();
        for bound in params.get(1)?.list() {
            let ring = self.bound(bound)?;
            if self.doc.kind(bound.reference()?) == "FACE_OUTER_BOUND" {
                outer.push(ring);
            } else {
                inner.push(ring);
            }
        }
        outer.extend(inner);
        let mut loops = outer;
        let mut forward = params.get(3).is_none_or(P::truth);
        if flip {
            forward = !forward;
            for ring in &mut loops {
                ring.reverse();
                for (_, f) in ring.iter_mut() {
                    *f = !*f;
                }
            }
        }
        Some(FaceSpec {
            surface,
            forward,
            loops,
        })
    }

    /// A shell's faces; an oriented shell set against its faces flips them.
    fn shell(&mut self, p: &P) -> Option<Vec<FaceSpec>> {
        let mut id = p.reference()?;
        let mut flip = false;
        if self.doc.kind(id) == "ORIENTED_CLOSED_SHELL"
            || self.doc.kind(id) == "ORIENTED_OPEN_SHELL"
        {
            let (_, params) = self.doc.simple(id)?;
            flip = !params.get(3).is_none_or(P::truth);
            id = params.get(2)?.reference()?;
        }
        let (_, params) = self.doc.simple(id)?;
        params
            .get(1)?
            .list()
            .iter()
            .map(|face| self.face(face, flip))
            .collect()
    }
}

/// A 3×4 placement: the columns are the x, y and z axes and the origin.
pub type Placement = [[f64; 4]; 3];

const IDENTITY: Placement = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// A body with the colours the file shows it in.
pub struct StepBody {
    pub body: kernel::brep::Body,
    /// The colour of the solid (or shell model) as a whole.
    pub colour: Option<Rgb>,
    /// Each face's own colour, in the body's face order.
    pub faces: Vec<Option<Rgb>>,
}

/// One product: its own bodies and where each of its components sits.
pub struct StepProduct {
    pub name: String,
    /// A representation of a file without products: the block is named
    /// after the file and this entity number (`<file>-<number>`).
    pub numbered: Option<u64>,
    pub bodies: Vec<StepBody>,
    /// The scale its bodies take in its block (1 but for a representation
    /// mapped without length units; see `without_products`).
    pub scale: f64,
    /// Components: the product (an index into [`StepModel::products`]) and
    /// its placement in this one.
    pub children: Vec<(usize, Placement)>,
}

/// What a STEP file holds.
pub struct StepModel {
    /// Millimetres per file length unit; `None` when the file names no
    /// length unit (its numbers are then taken as drawing units).
    pub unit_mm: Option<f64>,
    /// Solids that go straight into the drawing: those of a file without
    /// products whose top representation places nothing else.
    pub direct: Vec<StepBody>,
    pub products: Vec<StepProduct>,
    /// The products no other one is a component of.
    pub roots: Vec<usize>,
}

/// A named unit's size: millimetres for a length, radians for an angle.
/// A conversion-based unit is a multiple of another unit; the hop limit
/// stops units defined in terms of each other.
fn unit_size(doc: &Doc, mut id: u64) -> Option<f64> {
    let mut factor = 1.0;
    for _ in 0..16 {
        let Some(si) = doc.part(id, "SI_UNIT") else {
            let conversion = doc.part(id, "CONVERSION_BASED_UNIT")?;
            let (_, measure) = doc.simple(conversion.get(1)?.reference()?)?;
            factor *= measure.first()?.num()?;
            id = measure.get(1)?.reference()?;
            continue;
        };
        let prefix = match si.first() {
            Some(P::Enum(prefix)) => match prefix.as_str() {
                "MILLI" => 1e-3,
                "CENTI" => 1e-2,
                "DECI" => 1e-1,
                "KILO" => 1e3,
                "MICRO" => 1e-6,
                _ => 1.0,
            },
            _ => 1.0,
        };
        return Some(
            factor
                * match si.get(1) {
                    Some(P::Enum(name)) if name == "METRE" => prefix * 1000.0,
                    _ => prefix,
                },
        );
    }
    None
}

/// The millimetres of the length unit a representation context assigns,
/// or `None` when it assigns none.
fn length_unit_mm(doc: &Doc, context: u64) -> Option<f64> {
    doc.part(context, "GLOBAL_UNIT_ASSIGNED_CONTEXT")
        .and_then(|units| units.first())
        .map(P::list)
        .unwrap_or_default()
        .iter()
        .filter_map(P::reference)
        .find(|unit| doc.part(*unit, "LENGTH_UNIT").is_some())
        .map(|unit| unit_size(doc, unit).unwrap_or(1.0))
}

/// The bodies of one B-rep item: a solid, a solid with voids, or each shell
/// of a shell model.
fn item_bodies(doc: &Doc, id: u64) -> Vec<StepBody> {
    let Some((kind, params)) = doc.simple(id) else {
        return Vec::new();
    };
    let lumps: Vec<Vec<&P>> = match kind {
        "MANIFOLD_SOLID_BREP" => vec![params.get(1).into_iter().collect()],
        "BREP_WITH_VOIDS" => vec![params
            .get(1)
            .into_iter()
            .chain(params.get(2).map(P::list).unwrap_or_default())
            .collect()],
        "SHELL_BASED_SURFACE_MODEL" => params
            .get(1)
            .map(P::list)
            .unwrap_or_default()
            .iter()
            .map(|shell| vec![shell])
            .collect(),
        _ => return Vec::new(),
    };
    let mut bodies = Vec::new();
    for lump in lumps {
        let mut assembly = Assembly {
            doc,
            build: Builder::new(),
            vertices: HashMap::new(),
            edges: HashMap::new(),
            faces: Vec::new(),
        };
        let faces: Option<Vec<_>> = lump.iter().map(|shell| assembly.shell(shell)).collect();
        if let Some(faces) = faces {
            if assembly.build.lump(faces).is_some() {
                // The item's own style, or its (first) shell's.
                let colour = doc.styles.get(&id).copied().or_else(|| {
                    lump.iter()
                        .filter_map(|shell| shell.reference())
                        .find_map(|shell| doc.styles.get(&shell).copied())
                });
                bodies.push(StepBody {
                    body: assembly.build.body,
                    colour,
                    faces: assembly
                        .faces
                        .iter()
                        .map(|face| doc.styles.get(face).copied())
                        .collect(),
                });
            }
        }
    }
    bodies
}

impl Doc {
    /// Where a mapped item puts its representation: an axis placement, or
    /// a Cartesian transformation operator (axes, origin, uniform scale).
    fn target_frame(&self, id: u64) -> Option<Placement> {
        if self.kind(id) != "CARTESIAN_TRANSFORMATION_OPERATOR_3D" {
            return self.frame(id);
        }
        let (_, p) = self.simple(id)?;
        let axis = |k: usize| p.get(k).and_then(|d| self.direction(d)).map(Vec3::from);
        let x = axis(3)
            .and_then(Vec3::normalize)
            .unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        let z = axis(7)
            .and_then(Vec3::normalize)
            .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        let y = axis(4)
            .and_then(Vec3::normalize)
            .or_else(|| z.cross(x).normalize())
            .unwrap_or(Vec3::new(0.0, 1.0, 0.0));
        let o = p.get(5).and_then(|o| self.point(o)).unwrap_or([0.0; 3]);
        // Its scale is not applied (as the reference reads it).
        let [x, y, z] = [x, y, z].map(|v| [v.x, v.y, v.z]);
        Some([0, 1, 2].map(|r| [x[r], y[r], z[r], o[r]]))
    }

    /// An axis placement as a placement matrix.
    fn frame(&self, id: u64) -> Option<Placement> {
        let plane = self.placement(&P::Ref(id))?;
        let z = plane.normal()?;
        let [x, y, o] = [plane.x_axis, plane.y_axis, plane.origin];
        Some([0, 1, 2].map(|r| [x[r], y[r], z[r], o[r]]))
    }
}

/// `a·b`.
pub(super) fn compose(a: &Placement, b: &Placement) -> Placement {
    let mut out = [[0.0; 4]; 3];
    for r in 0..3 {
        for c in 0..4 {
            out[r][c] =
                (0..3).map(|k| a[r][k] * b[k][c]).sum::<f64>() + if c == 3 { a[r][3] } else { 0.0 };
        }
    }
    out
}

/// The inverse of a rigid placement.
fn invert(a: &Placement) -> Placement {
    let mut out = [[0.0; 4]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = a[c][r];
        }
        out[r][3] = -(0..3).map(|k| a[k][r] * a[k][3]).sum::<f64>();
    }
    out
}

pub fn read(bytes: &[u8]) -> Option<StepModel> {
    let text = String::from_utf8_lossy(bytes);
    if !text.trim_start().starts_with("ISO-10303-21") {
        return None;
    }
    let data = text.find("DATA;")? + 5;
    let mut cursor = Cursor {
        s: &text.as_bytes()[data..],
        i: 0,
        depth: 0,
        buf: Vec::new(),
    };
    let mut instances = HashMap::new();
    while cursor.peek() == Some(b'#') {
        match cursor.instance() {
            Some((id, parts)) => {
                instances.insert(id, parts);
            }
            None => {
                // Skip a malformed instance up to the next one.
                while cursor.s.get(cursor.i).is_some_and(|c| *c != b';') {
                    cursor.i += 1;
                }
                // Past the `;`, or at the end of a truncated file.
                cursor.i = (cursor.i + 1).min(cursor.s.len());
            }
        }
    }
    let mut doc = Doc {
        instances,
        angle: 1.0,
        styles: HashMap::new(),
    };

    let mut unit_mm = None;
    let mut angle = 1.0;
    let mut ids: Vec<u64> = doc.instances.keys().copied().collect();
    ids.sort_unstable();
    for &id in &ids {
        let Some(units) = doc.part(id, "GLOBAL_UNIT_ASSIGNED_CONTEXT") else {
            continue;
        };
        for unit in units.first().map(P::list).unwrap_or_default() {
            let Some(unit) = unit.reference() else {
                continue;
            };
            if doc.part(unit, "LENGTH_UNIT").is_some() {
                unit_mm = Some(unit_size(&doc, unit).unwrap_or(1.0));
            } else if doc.part(unit, "PLANE_ANGLE_UNIT").is_some() {
                angle = unit_size(&doc, unit).unwrap_or(1.0);
            }
        }
        break;
    }
    doc.angle = angle;
    // Styled items: plain ones first, so an overriding style wins.
    let mut styled = Vec::new();
    for kind in ["STYLED_ITEM", "OVER_RIDING_STYLED_ITEM"] {
        for &id in &ids {
            if doc.kind(id) != kind {
                continue;
            }
            let Some((_, params)) = doc.simple(id) else {
                continue;
            };
            let (Some(styles), Some(item)) = (params.get(1), params.get(2).and_then(P::reference))
            else {
                continue;
            };
            if let Some(colour) = doc.style_colour(styles) {
                styled.push((item, colour));
            }
        }
    }
    doc.styles.extend(styled);
    let doc = doc;

    // What each instance id is, by kind.
    let of_kind = |kind: &str| -> Vec<u64> {
        ids.iter()
            .copied()
            .filter(|&id| doc.kind(id) == kind)
            .collect()
    };
    let param = |id: u64, k: usize| {
        doc.simple(id)
            .and_then(|(_, p)| p.get(k))
            .and_then(P::reference)
    };

    // Product definitions and the name of the product each defines.
    let definitions: Vec<u64> = ids
        .iter()
        .copied()
        .filter(|&id| {
            matches!(
                doc.kind(id),
                "PRODUCT_DEFINITION" | "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS"
            )
        })
        .collect();
    let product_name = |definition: u64| -> String {
        let product = param(definition, 2).and_then(|formation| param(formation, 2));
        product
            .and_then(|p| doc.simple(p))
            .and_then(|(_, params)| {
                params
                    .get(1)
                    .and_then(P::text)
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| params.first().and_then(P::text))
            })
            .unwrap_or_default()
            .to_string()
    };
    if definitions.is_empty() {
        return Some(without_products(&doc, &ids, unit_mm));
    }
    // The shape of a definition (or an occurrence), and the representations
    // attached to a shape.
    let shape_of: HashMap<u64, u64> = of_kind("PRODUCT_DEFINITION_SHAPE")
        .into_iter()
        .filter_map(|pds| Some((param(pds, 2)?, pds)))
        .collect();
    let mut reps_of_shape: HashMap<u64, Vec<u64>> = HashMap::new();
    for sdr in of_kind("SHAPE_DEFINITION_REPRESENTATION") {
        if let (Some(pds), Some(rep)) = (param(sdr, 0), param(sdr, 1)) {
            reps_of_shape.entry(pds).or_default().push(rep);
        }
    }
    // Representations tied without a transformation share their items.
    let mut linked: HashMap<u64, Vec<u64>> = HashMap::new();
    for id in &ids {
        let Some(("SHAPE_REPRESENTATION_RELATIONSHIP", params)) = doc.simple(*id) else {
            continue;
        };
        if let (Some(a), Some(b)) = (
            params.get(2).and_then(P::reference),
            params.get(3).and_then(P::reference),
        ) {
            linked.entry(a).or_default().push(b);
            linked.entry(b).or_default().push(a);
        }
    }
    let items_of = |rep: u64| -> Vec<u64> {
        let mut seen = vec![rep];
        let mut at = 0;
        while at < seen.len() {
            for next in linked.get(&seen[at]).into_iter().flatten() {
                if !seen.contains(next) {
                    seen.push(*next);
                }
            }
            at += 1;
        }
        seen.iter()
            .filter_map(|r| doc.simple(*r))
            .flat_map(|(_, params)| params.get(1).map(P::list).unwrap_or_default())
            .filter_map(P::reference)
            .collect()
    };

    let mut index: HashMap<u64, usize> = HashMap::new();
    let mut products: Vec<StepProduct> = Vec::new();
    let mut used_items: HashSet<u64> = HashSet::new();
    for &definition in &definitions {
        let mut bodies = Vec::new();
        for rep in shape_of
            .get(&definition)
            .and_then(|pds| reps_of_shape.get(pds))
            .into_iter()
            .flatten()
        {
            for item in items_of(*rep) {
                if !used_items.contains(&item) {
                    let found = item_bodies(&doc, item);
                    if !found.is_empty() {
                        used_items.insert(item);
                    }
                    bodies.extend(found);
                }
            }
        }
        index.insert(definition, products.len());
        products.push(StepProduct {
            name: product_name(definition),
            numbered: None,
            scale: 1.0,
            bodies,
            children: Vec::new(),
        });
    }

    // Components, placed by the transformation of their occurrence.
    // The first context-dependent shape representation of each product
    // definition shape, by entity number — the one the placement lookup
    // below picks; indexed here so each occurrence is found in O(1)
    // instead of a scan over every id in the file.
    let mut cdsr_of_shape: HashMap<u64, u64> = HashMap::new();
    for cdsr in of_kind("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION") {
        if let Some(pds) = param(cdsr, 1) {
            cdsr_of_shape.entry(pds).or_insert(cdsr);
        }
    }
    let mut component = vec![false; products.len()];
    for occurrence in of_kind("NEXT_ASSEMBLY_USAGE_OCCURRENCE") {
        let (Some(&parent), Some(&child)) = (
            param(occurrence, 3).and_then(|d| index.get(&d)),
            param(occurrence, 4).and_then(|d| index.get(&d)),
        ) else {
            continue;
        };
        let placement = shape_of
            .get(&occurrence)
            .and_then(|pds| cdsr_of_shape.get(pds).copied())
            .and_then(|cdsr| param(cdsr, 0))
            .and_then(|relation| {
                doc.part(relation, "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")
            })
            .and_then(|p| p.first()?.reference())
            .and_then(|transformation| {
                let (_, t) = doc.simple(transformation)?;
                let from = doc.frame(t.get(2)?.reference()?)?;
                let to = doc.frame(t.get(3)?.reference()?)?;
                Some(compose(&to, &invert(&from)))
            })
            .unwrap_or(IDENTITY);
        products[parent].children.push((child, placement));
        component[child] = true;
    }

    // A file whose solids no product claims: one product holding them all,
    // named after the file's first product.
    let loose: Vec<u64> = ids
        .iter()
        .copied()
        .filter(|id| !used_items.contains(id))
        .collect();
    let mut loose_bodies = Vec::new();
    for id in loose {
        loose_bodies.extend(item_bodies(&doc, id));
    }
    if !loose_bodies.is_empty() {
        let name = definitions
            .first()
            .map(|d| product_name(*d))
            .unwrap_or_default();
        component.push(false);
        products.push(StepProduct {
            name,
            numbered: None,
            scale: 1.0,
            bodies: loose_bodies,
            children: Vec::new(),
        });
    }

    let roots = (0..products.len())
        .filter(|&k| {
            !component[k] && (!products[k].bodies.is_empty() || !products[k].children.is_empty())
        })
        .collect();
    Some(StepModel {
        unit_mm,
        direct: Vec::new(),
        products,
        roots,
    })
}

/// A file without products: each representation holding solids or placing
/// other representations (mapped items) is a block, a mapped item an insert
/// of the representation it maps. A top representation (one no other maps)
/// that places nothing puts its solids straight into the drawing.
fn without_products(doc: &Doc, ids: &[u64], unit_mm: Option<f64>) -> StepModel {
    let param = |id: u64, k: usize| {
        doc.simple(id)
            .and_then(|(_, p)| p.get(k))
            .and_then(P::reference)
    };
    let reps: Vec<u64> = ids
        .iter()
        .copied()
        .filter(|&id| {
            let kind = doc.kind(id);
            kind.ends_with("REPRESENTATION") && kind != "SHAPE_DEFINITION_REPRESENTATION"
        })
        .collect();
    let mut index: HashMap<u64, usize> = HashMap::new();
    let mut products: Vec<StepProduct> = Vec::new();
    let mut mapped: Vec<(usize, u64, Placement)> = Vec::new();
    for &rep in &reps {
        // The items list: the second parameter, or the first where an
        // early writer left the name out.
        let items: Vec<u64> = doc
            .simple(rep)
            .and_then(|(_, p)| p.iter().find(|v| matches!(v, P::List(_))))
            .map(P::list)
            .unwrap_or_default()
            .iter()
            .filter_map(P::reference)
            .collect();
        let mut bodies = Vec::new();
        for &item in &items {
            if doc.kind(item) == "MAPPED_ITEM" {
                // MAPPED_ITEM(name, map, target): the map's representation,
                // moved from the map's origin to the target.
                let Some(map) = param(item, 1) else { continue };
                let Some(source) = param(map, 1) else {
                    continue;
                };
                let from = param(map, 0).and_then(|o| doc.frame(o)).unwrap_or(IDENTITY);
                let to = param(item, 2)
                    .and_then(|t| doc.target_frame(t))
                    .unwrap_or(IDENTITY);
                mapped.push((products.len(), source, compose(&to, &invert(&from))));
            } else {
                bodies.extend(item_bodies(doc, item));
            }
        }
        index.insert(rep, products.len());
        products.push(StepProduct {
            name: String::new(),
            numbered: Some(rep),
            scale: 1.0,
            bodies,
            children: Vec::new(),
        });
    }
    // A representation one without length units maps, itself without
    // them, comes in at a hundredth of its size (as the reference reads it).
    let context = |rep: u64| {
        doc.simple(rep)
            .and_then(|(_, p)| p.last())
            .and_then(P::reference)
    };
    let unit = |rep: u64| context(rep).and_then(|c| length_unit_mm(doc, c));
    let unitless = |rep: u64| unit(rep).is_none();
    let parents: HashMap<usize, u64> = index.iter().map(|(rep, k)| (*k, *rep)).collect();
    let mut component = vec![false; products.len()];
    for (parent, source, placement) in mapped {
        if let Some(&child) = index.get(&source) {
            if unitless(source) && parents.get(&parent).is_some_and(|rep| unitless(*rep)) {
                products[child].scale = 0.01;
            }
            products[parent].children.push((child, placement));
            component[child] = true;
        }
    }
    // The drawing takes the length unit of the top representation (the
    // first one no other maps), as the reference reads it.
    let mut top_unit = None;
    let mut direct = Vec::new();
    let mut roots = Vec::new();
    for k in 0..products.len() {
        if component[k] || (products[k].bodies.is_empty() && products[k].children.is_empty()) {
            continue;
        }
        if direct.is_empty() && roots.is_empty() {
            top_unit = Some(parents.get(&k).and_then(|rep| unit(*rep)));
        }
        if products[k].children.is_empty() {
            direct.append(&mut products[k].bodies);
        } else {
            roots.push(k);
        }
    }
    StepModel {
        unit_mm: top_unit.unwrap_or(unit_mm),
        direct,
        products,
        roots,
    }
}
