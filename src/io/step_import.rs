//! STEP (ISO 10303-21) reading: the B-rep solids and shell models of an
//! AP203/AP214/AP242 file, read into Solidcore records by Solidcore's STEP reader, with the
//! product structure, names, colours and the file's length unit read here.

use std::collections::{HashMap, HashSet};

use ocs_geom::space::{Plane, Vec3};

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
    /// Parameters read so far, every list item counted.
    params: usize,
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
        self.params += 1;
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
                // A complex instance of more parts than any schema combines.
                if name.is_empty() || parts.len() >= 64 {
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
    /// The file as Solidcore's reader reads it, for the bodies.
    exchange: Option<solidcore_record::step::p21::Exchange>,
    /// Plane-angle unit in radians.
    angle: f64,
    /// The colour each styled item (a solid, a shell or a face) is shown
    /// in, as 0–1 RGB.
    styles: HashMap<u64, [f64; 3]>,
    /// The distance uncertainty of the file's context, in its length unit:
    /// how far apart two points may be and still be one.
    tolerance: Option<f64>,
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
        let (kind, params) = self.simple(p.reference()?)?;
        let origin = self.point(params.get(1)?)?;
        // A 2D placement (in a surface's parameter space) has no axis: its
        // second attribute is the reference direction.
        let planar = kind == "AXIS2_PLACEMENT_2D";
        let z = params
            .get(2)
            .filter(|_| !planar)
            .and_then(|d| self.direction(d))
            .unwrap_or([0.0, 0.0, 1.0]);
        let x = params
            .get(if planar { 2 } else { 3 })
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

}

/// Highest B-spline degree read; a file's larger one is refused rather than
/// sizing a knot vector from it.
/// A 3×4 placement: the columns are the x, y and z axes and the origin.
pub type Placement = [[f64; 4]; 3];

const IDENTITY: Placement = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// A body with the colours the file shows it in.
pub struct StepBody {
    pub body: ocs_solid::model::Brep,
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

/// The distance uncertainty a representation context assigns, in the
/// context's length unit (`unit_mm` millimetres): its value in its own unit,
/// which need not be the context's.
fn uncertainty(doc: &Doc, context: u64, unit_mm: f64) -> Option<f64> {
    let measures = doc.part(context, "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT")?;
    measures.first()?.list().iter().find_map(|measure| {
        let (kind, params) = doc.simple(measure.reference()?)?;
        if kind != "UNCERTAINTY_MEASURE_WITH_UNIT" {
            return None;
        }
        let value = params.first()?.num()?;
        let own = params
            .get(1)
            .and_then(P::reference)
            .and_then(|unit| unit_size(doc, unit))
            .unwrap_or(unit_mm);
        let tolerance = value * own / unit_mm;
        (tolerance.is_finite() && tolerance > 0.0).then_some(tolerance)
    })
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

/// The body of one B-rep item (a solid, a solid with voids, or a shell model), read by
/// Solidcore's STEP reader in the file's own units; a solid whose shell the file leaves open is
/// read as sheets.
fn item_bodies(doc: &Doc, id: u64) -> Vec<StepBody> {
    let Some(exchange) = doc.exchange.as_ref() else {
        return Vec::new();
    };
    // Only the representation items that are bodies: a solid, or a model of sheets.
    let sheets = match doc.kind(id) {
        "MANIFOLD_SOLID_BREP" | "FACETED_BREP" | "BREP_WITH_VOIDS" => false,
        "SHELL_BASED_SURFACE_MODEL" => true,
        _ => return Vec::new(),
    };
    let context = solidcore_step::units::Context::default();
    let trims = ocs_solid::geometry::Traced;
    let solid = if sheets {
        solidcore_step::topology::read_solid_as_sheets(exchange, id, context, &trims)
    } else {
        solidcore_step::topology::read_solid_with(exchange, id, context, &trims)
            .or_else(|_| solidcore_step::topology::read_solid_as_sheets(exchange, id, context, &trims))
    };
    let Ok(mut solid) = solid else {
        return Vec::new();
    };
    // Faces written each with edges of their own: sewn where they meet.
    if !sheets && !ocs_solid::model::is_solid(&solid.brep) {
        let tolerance = doc.tolerance.unwrap_or(1e-6).max(1e-9);
        if let Ok(sewn) = ocs_solid::model::sewn(&[&solid.brep], tolerance) {
            if ocs_solid::model::is_solid(&sewn) {
                solid.brep = sewn;
            }
        }
    }
    // The item's own style, or its (first) shell's.
    let colour = doc.styles.get(&id).copied().or_else(|| {
        doc.simple(id)
            .and_then(|(_, params)| params.get(1))
            .into_iter()
            .flat_map(|p| match p {
                P::List(items) => items.iter().filter_map(P::reference).collect::<Vec<_>>(),
                other => other.reference().into_iter().collect(),
            })
            .find_map(|shell| doc.styles.get(&shell).copied())
    });
    vec![StepBody {
        faces: solid.face_origin.iter().map(|face| doc.styles.get(face).copied()).collect(),
        body: solid.brep,
        colour,
    }]
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

/// The instances of a file's DATA section, read leniently: a malformed
/// instance is skipped. The input is hostile, so the text's size, the
/// instance and parameter counts and a complex instance's parts are capped
/// (as the Part 21 parser solidcore vendors caps them) and the whole file is
/// refused past a cap; nothing is allocated from a count read out of the
/// file.
fn instances(text: &str) -> Option<HashMap<u64, Instance>> {
    use solidcore_record::step::p21::{MAX_BYTES, MAX_ENTITIES, MAX_PARAMS};
    if text.len() > MAX_BYTES || !text.trim_start().starts_with("ISO-10303-21") {
        return None;
    }
    let data = text.find("DATA;")? + 5;
    let mut cursor = Cursor {
        s: &text.as_bytes()[data..],
        i: 0,
        depth: 0,
        buf: Vec::new(),
        params: 0,
    };
    let mut instances = HashMap::new();
    let mut read = 0usize;
    while cursor.peek() == Some(b'#') {
        read += 1;
        if read > MAX_ENTITIES || cursor.params > MAX_PARAMS {
            return None;
        }
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
    Some(instances)
}

pub fn read(bytes: &[u8]) -> Option<StepModel> {
    let text = String::from_utf8_lossy(bytes);
    let mut doc = Doc {
        instances: instances(&text)?,
        exchange: solidcore_record::step::p21::parse(&text).ok(),
        angle: 1.0,
        styles: HashMap::new(),
        tolerance: None,
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
        doc.tolerance = uncertainty(&doc, id, unit_mm.unwrap_or(1.0));
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

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_solid::model::r;

    const BOX: &[u8] = include_bytes!("../../tests/fixtures/step/box.step");
    const INCH_BOX: &[u8] = include_bytes!("../../tests/fixtures/step/inch_box.step");
    const CURVED: &[u8] = include_bytes!("../../tests/fixtures/step/curved.step");
    const NURBS_SPHERE: &[u8] = include_bytes!("../../tests/fixtures/step/nurbs_sphere.step");
    const ASSEMBLY: &[u8] = include_bytes!("../../tests/fixtures/step/assembly.step");
    const OPEN_BOX: &[u8] = include_bytes!("../../tests/fixtures/step/open_box.step");
    const COLOURED_BOX: &[u8] = include_bytes!("../../tests/fixtures/step/coloured_box.step");
    const NO_PRODUCTS: &[u8] = include_bytes!("../../tests/fixtures/step/no_products.step");

    fn bounds(body: &ocs_solid::model::Brep) -> ([f64; 3], [f64; 3]) {
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

    fn volume(body: &ocs_solid::model::Brep) -> f64 {
        ocs_solid::model::mass_properties(body)
            .expect("a closed body")
            .volume
    }

    fn assert_volume(body: &ocs_solid::model::Brep, expected: f64, what: &str) {
        let v = volume(body);
        assert!(
            (v - expected).abs() <= expected * 1e-6,
            "{what}: volume {v} != {expected}"
        );
    }

    /// The one product of a file holding one solid.
    fn only_product(model: &StepModel) -> &StepProduct {
        assert!(model.direct.is_empty());
        assert_eq!(model.roots.len(), 1);
        let product = &model.products[model.roots[0]];
        assert!(product.children.is_empty());
        assert_eq!(product.bodies.len(), 1);
        product
    }

    #[test]
    fn a_box_is_one_named_product_with_one_solid() {
        let model = read(BOX).expect("a STEP model");
        assert_eq!(model.unit_mm, Some(1.0));
        let product = only_product(&model);
        assert_eq!(product.name, "Box");
        assert_eq!((product.numbered, product.scale), (None, 1.0));
        let body = &product.bodies[0];
        assert_eq!(body.body.faces.len(), 6);
        assert_eq!(body.colour, None);
        let (min, max) = bounds(&body.body);
        assert_close(min, [0.0, 0.0, 0.0], 1e-9, "min");
        assert_close(max, [10.0, 20.0, 30.0], 1e-9, "max");
        assert_volume(&body.body, 6000.0, "box");
    }

    #[test]
    fn coordinates_stay_in_the_file_unit_it_names() {
        // A 1 × 2 × 3 inch box: the numbers are inches, and the unit says so;
        // scaling to the drawing is the caller's.
        let model = read(INCH_BOX).expect("a STEP model");
        assert_eq!(model.unit_mm, Some(25.4));
        let body = &only_product(&model).bodies[0].body;
        let (min, max) = bounds(body);
        assert_close(min, [0.0, 0.0, 0.0], 1e-9, "min");
        assert_close(max, [1.0, 2.0, 3.0], 1e-9, "max");
        assert_volume(body, 6.0, "inch box");
    }

    #[test]
    fn curved_solids_read_on_their_analytic_surfaces() {
        use std::f64::consts::PI;
        let model = read(CURVED).expect("a STEP model");
        let root = &model.products[model.roots[0]];
        assert!(root.bodies.is_empty());
        let children: Vec<&StepProduct> = root
            .children
            .iter()
            .map(|(k, _)| &model.products[*k])
            .collect();
        let names: Vec<&str> = children.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Sphere", "Cylinder", "Cone", "Torus"]);
        for (_, placement) in &root.children {
            assert_eq!(*placement, IDENTITY);
        }
        // (faces, min, max, volume)
        let expected = [
            (
                1,
                [-5.0, -5.0, -5.0],
                [5.0, 5.0, 5.0],
                4.0 / 3.0 * PI * 125.0,
            ),
            (3, [17.0, -3.0, 0.0], [23.0, 3.0, 10.0], PI * 9.0 * 10.0),
            (
                3,
                [36.0, -4.0, 0.0],
                [44.0, 4.0, 8.0],
                PI * 8.0 / 3.0 * (16.0 + 4.0 + 1.0),
            ),
            (
                1,
                [52.0, -8.0, -2.0],
                [68.0, 8.0, 2.0],
                2.0 * PI * PI * 6.0 * 4.0,
            ),
        ];
        for (product, (faces, min, max, v)) in children.iter().zip(expected) {
            assert_eq!(product.bodies.len(), 1, "{}", product.name);
            let body = &product.bodies[0].body;
            assert_eq!(body.faces.len(), faces, "{}", product.name);
            let (lo, hi) = bounds(body);
            assert_close(lo, min, 1e-6, &product.name);
            assert_close(hi, max, 1e-6, &product.name);
            assert_volume(body, v, &product.name);
        }
        let kinds: Vec<&str> = children
            .iter()
            .flat_map(|p| {
                p.bodies[0].body.faces.iter().map(|f| match &f.surface {
                    r::Surface::Plane { .. } | r::Surface::PlaneThrough { .. } => "plane",
                    r::Surface::Sphere { .. } => "sphere",
                    r::Surface::Cylinder { .. } => "cylinder",
                    r::Surface::Cone { .. } => "cone",
                    r::Surface::Torus { .. } => "torus",
                    _ => "other",
                })
            })
            .collect();
        for kind in ["sphere", "cylinder", "cone", "torus"] {
            assert!(kinds.contains(&kind), "{kind} in {kinds:?}");
        }
        assert!(!kinds.contains(&"other"), "{kinds:?}");
    }

    #[test]
    #[ignore = "solidcore-step refuses a spline face bounded by vertex loops alone"]
    fn a_rational_spline_surface_keeps_its_weights() {
        // A sphere of radius 5 written as a rational B-spline surface (a
        // complex entity): its volume only comes out right if the knots,
        // control points and weights were all read right.
        let model = read(NURBS_SPHERE).expect("a STEP model");
        let body = &only_product(&model).bodies[0].body;
        assert_eq!(body.faces.len(), 1);
        assert!(
            matches!(body.faces[0].parameters(), r::Surface::RationalSpline { .. }),
            "{:?}",
            body.faces[0].surface
        );
        assert_volume(body, 4.0 / 3.0 * std::f64::consts::PI * 125.0, "sphere");
    }

    #[test]
    fn an_assembly_places_its_components() {
        let model = read(ASSEMBLY).expect("a STEP model");
        assert_eq!(model.roots.len(), 1);
        let root = &model.products[model.roots[0]];
        assert_eq!(root.name, "Assembly");
        assert!(root.bodies.is_empty());
        assert_eq!(root.children.len(), 2);
        let child = |k: usize| (&model.products[root.children[k].0], root.children[k].1);
        let (plate, at_plate) = child(0);
        let (post, at_post) = child(1);
        assert_eq!((plate.name.as_str(), post.name.as_str()), ("Plate", "Post"));
        assert_eq!(at_plate, IDENTITY);
        // Turned 90° about z, then moved 50 along x.
        let expected = [
            [0.0, -1.0, 0.0, 50.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        for r in 0..3 {
            for c in 0..4 {
                assert!((at_post[r][c] - expected[r][c]).abs() < 1e-9, "{at_post:?}");
            }
        }
        // Each component's solid is in its own frame.
        let (min, max) = bounds(&plate.bodies[0].body);
        assert_close(min, [0.0, 0.0, 0.0], 1e-9, "plate min");
        assert_close(max, [10.0, 20.0, 2.0], 1e-9, "plate max");
        let (min, max) = bounds(&post.bodies[0].body);
        assert_close(min, [0.0, 0.0, 0.0], 1e-9, "post min");
        assert_close(max, [2.0, 2.0, 15.0], 1e-9, "post max");
        // Neither component is a root of its own.
        assert!(!model.roots.contains(&root.children[0].0));
        assert!(!model.roots.contains(&root.children[1].0));
    }

    #[test]
    fn a_shell_model_reads_as_an_open_body() {
        let model = read(OPEN_BOX).expect("a STEP model");
        let product = only_product(&model);
        assert_eq!(product.name, "OpenBox");
        let body = &product.bodies[0].body;
        assert_eq!(body.faces.len(), 5, "a box without its top");
        let (min, max) = bounds(body);
        assert_close(min, [0.0, 0.0, 0.0], 1e-9, "min");
        assert_close(max, [10.0, 10.0, 10.0], 1e-9, "max");
    }

    #[test]
    fn solid_and_face_colours_come_from_their_styles() {
        let model = read(COLOURED_BOX).expect("a STEP model");
        let body = &only_product(&model).bodies[0];
        assert_eq!(body.colour, Some([0.8, 0.2, 0.1]), "the solid's own RGB");
        assert_eq!(body.faces.len(), 6);
        assert_eq!(body.faces[0], Some([0.0, 0.0, 1.0]), "a predefined colour");
        assert_eq!(
            body.faces[1],
            Some([1.0, 1.0, 0.0]),
            "an overriding style wins"
        );
        assert_eq!(body.faces[2], Some([0.25, 0.5, 0.75]), "a rendering style");
        assert_eq!(&body.faces[3..], &[None, None, None]);
        // The styles change nothing else.
        assert_volume(&body.body, 6000.0, "coloured box");
    }

    #[test]
    fn a_file_without_products_puts_its_solids_straight_in() {
        let model = read(NO_PRODUCTS).expect("a STEP model");
        assert_eq!(model.unit_mm, Some(1.0));
        assert!(model.roots.is_empty());
        assert_eq!(model.direct.len(), 1);
        let body = &model.direct[0].body;
        assert_eq!(body.faces.len(), 6);
        let (min, max) = bounds(body);
        assert_close(min, [0.0, 0.0, 0.0], 1e-9, "min");
        assert_close(max, [10.0, 20.0, 30.0], 1e-9, "max");
    }

    #[test]
    fn input_that_is_not_step_reads_as_nothing() {
        assert!(read(b"").is_none());
        assert!(read(b"not a STEP file").is_none());
        assert!(read(b"ISO-10303-21;\nHEADER;\nENDSEC;\nEND-ISO-10303-21;\n").is_none());
        // A data section with no shape is an empty model.
        let model = read(b"ISO-10303-21;\nDATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\nENDSEC;\n")
            .expect("a STEP file");
        assert!(model.direct.is_empty() && model.roots.is_empty());
    }

    /// Damaged copies of every fixture: cut at every third line, each line
    /// left out, and every fourth number replaced by a value a careless
    /// writer might leave there. None may panic.
    #[test]
    fn damaged_files_never_panic() {
        const VALUES: [&str; 6] = ["0.", "-1.", "1.E308", "-1.E308", "1.E-308", "99999999"];
        let mut cases = 0;
        for bytes in [
            BOX,
            INCH_BOX,
            CURVED,
            NURBS_SPHERE,
            ASSEMBLY,
            OPEN_BOX,
            COLOURED_BOX,
            NO_PRODUCTS,
        ] {
            let text = std::str::from_utf8(bytes).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            let mut check = |damaged: String| {
                cases += 1;
                let _ = read(damaged.as_bytes());
            };
            for n in 0..lines.len() {
                if n % 3 == 0 {
                    check(lines[..n].join("\n"));
                }
                check([&lines[..n], &lines[n + 1..]].concat().join("\n"));
            }
            let mut value = 0;
            let mut number = 0;
            for (n, line) in lines.iter().enumerate() {
                if !line.starts_with('#') {
                    continue;
                }
                // Each number after the instance's `=`.
                let Some(eq) = line.find('=') else { continue };
                let mut start = None;
                for (i, c) in line.char_indices().skip(eq + 1).chain([(line.len(), ',')]) {
                    let numeric = c.is_ascii_digit() || matches!(c, '.' | '-' | 'E' | '+');
                    match (start, numeric) {
                        (None, true) if !line[..i].ends_with('#') => start = Some(i),
                        (Some(s), false) => {
                            number += 1;
                            start = None;
                            if number % 4 != 0 {
                                continue;
                            }
                            let replaced = format!(
                                "{}{}{}",
                                &line[..s],
                                VALUES[value % VALUES.len()],
                                &line[i..]
                            );
                            value += 1;
                            let mut damaged = lines.clone();
                            damaged[n] = &replaced;
                            check(damaged.join("\n"));
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(cases > 1000, "{cases} damaged files");
    }

    /// A whole sphere bounded by a vertex loop gets its seam and meshes
    /// (#3051).
    #[test]
    fn a_sphere_on_a_vertex_loop_gets_a_seam_and_meshes() {
        let curved = read(CURVED).expect("a STEP model");
        let analytic = &curved.products[curved.products[curved.roots[0]].children[0].0];
        // ponytail: the spline-written sphere is refused by solidcore-step, which reads a face
        // bounded by vertex loops alone only on an analytic sphere.
        for body in [&analytic.bodies[0].body] {
            assert!(ocs_solid::model::check(body).is_empty(), "{:?}", ocs_solid::model::check(body));
            let mesh = crate::scene::model::solid_model::tessellate(
                body,
                crate::scene::model::solid_model::DisplayTolerance::new(0.1),
            );
            assert!(mesh.is_some_and(|mesh| !mesh.triangles.is_empty()), "the sphere meshes");
        }
    }

    /// A file's DATA section read on its own.
    fn doc(data: &str) -> Doc {
        let text = format!("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n");
        Doc {
            instances: instances(&text).expect("instances"),
            angle: 1.0,
            styles: HashMap::new(),
            tolerance: None,
            exchange: None,
        }
    }

    /// A STEP file of one solid, its context in millimetres.
    fn solid_file(data: &str, solid: u64) -> String {
        format!(
            "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n{data}\n\
             #9901=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
             #9902=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-6),#9901,'distance_accuracy_value','');\n\
             #9903=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#9902)) \
             GLOBAL_UNIT_ASSIGNED_CONTEXT((#9901)) REPRESENTATION_CONTEXT('',''));\n\
             #9904=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{solid}),#9903);\n\
             ENDSEC;\nEND-ISO-10303-21;\n"
        )
    }

    fn only_body(text: &str) -> ocs_solid::model::Brep {
        let model = read(text.as_bytes()).expect("a STEP model");
        let mut bodies: Vec<_> = model.direct.into_iter().map(|b| b.body).collect();
        for product in model.products {
            bodies.extend(product.bodies.into_iter().map(|b| b.body));
        }
        assert_eq!(bodies.len(), 1);
        bodies.pop().unwrap()
    }

    /// The exported 5 × 10 cylinder with its side's surface written as
    /// `surface` (`#S` standing for the cylinder's placement) and `extra`
    /// instances added.
    fn cylinder_side_as(surface: &str, extra: &str) -> String {
        let item = crate::io::step::StepItem {
            body: ocs_solid::model::make_cylinder([0.0, 0.0, 0.0], 5.0, 10.0).ok(),
            solid: true,
            mesh: None,
            name: String::new(),
            layer: None,
            color: None,
        };
        let text = crate::io::step::build_step(&[item], 1.0, crate::io::step::Schema::Ap214, "p")
            .expect("written");
        let line = text
            .lines()
            .find(|line| line.contains("=CYLINDRICAL_SURFACE("))
            .expect("a cylinder");
        let (id, rest) = line.split_once('=').unwrap();
        let placement = rest.split(['#', ',']).nth(2).unwrap();
        let placement = format!("#{placement}");
        let text = text.replace(line, &format!("{id}={};", surface.replace("#S", &placement)));
        let extra = extra.replace("#S", &placement);
        text.replace("ENDSEC;\nEND-ISO", &format!("{extra}\nENDSEC;\nEND-ISO"))
    }

    const CYLINDER: f64 = std::f64::consts::PI * 25.0 * 10.0;

    fn assert_near(body: &ocs_solid::model::Brep, expected: f64, what: &str) {
        assert!(ocs_solid::model::check(body).is_empty(), "{what}: {:?}", ocs_solid::model::check(body));
        let v = volume(body);
        assert!((v - expected).abs() <= expected * 1e-3, "{what}: volume {v} != {expected}");
    }

    #[test]
    fn a_surface_of_revolution_reads_as_a_spline() {
        let text = cylinder_side_as(
            "SURFACE_OF_REVOLUTION('',#9001,#9006)",
            "#9000=CARTESIAN_POINT('',(5.,0.,0.));\n\
             #9002=DIRECTION('',(0.,0.,1.));\n\
             #9003=VECTOR('',#9002,1.);\n\
             #9001=LINE('',#9000,#9003);\n\
             #9005=CARTESIAN_POINT('',(0.,0.,0.));\n\
             #9006=AXIS1_PLACEMENT('',#9005,#9002);",
        );
        assert_near(&only_body(&text), CYLINDER, "revolution");
    }

    #[test]
    fn a_surface_of_linear_extrusion_reads_as_a_spline() {
        let text = cylinder_side_as(
            "SURFACE_OF_LINEAR_EXTRUSION('',#9012,#9003)",
            "#9002=DIRECTION('',(0.,0.,1.));\n\
             #9003=VECTOR('',#9002,1.);\n\
             #9005=CARTESIAN_POINT('',(0.,0.,0.));\n\
             #9011=DIRECTION('',(1.,0.,0.));\n\
             #9010=AXIS2_PLACEMENT_3D('',#9005,#9002,#9011);\n\
             #9012=CIRCLE('',#9010,5.);",
        );
        assert_near(&only_body(&text), CYLINDER, "extrusion");
    }

    #[test]
    fn an_offset_and_a_trimmed_surface_read_as_their_basis() {
        let offset = cylinder_side_as(
            "OFFSET_SURFACE('',#9020,1.,.F.)",
            "#9020=CYLINDRICAL_SURFACE('',#S,4.);",
        );
        assert_near(&only_body(&offset), CYLINDER, "offset");
        let trimmed = cylinder_side_as(
            "RECTANGULAR_TRIMMED_SURFACE('',#9030,0.,6.3,0.,10.,.T.,.T.)",
            "#9030=CYLINDRICAL_SURFACE('',#S,5.);",
        );
        assert_near(&only_body(&trimmed), CYLINDER, "trimmed");
    }

    #[test]
    fn a_faceted_brep_reads_its_polygons() {
        let text = solid_file(
            "#1=CARTESIAN_POINT('',(0.,0.,0.));\n\
             #2=CARTESIAN_POINT('',(1.,0.,0.));\n\
             #3=CARTESIAN_POINT('',(0.,1.,0.));\n\
             #4=CARTESIAN_POINT('',(0.,0.,1.));\n\
             #11=POLY_LOOP('',(#1,#3,#2));\n#12=FACE_OUTER_BOUND('',#11,.T.);\n#13=FACE('',(#12));\n\
             #14=POLY_LOOP('',(#1,#2,#4));\n#15=FACE_OUTER_BOUND('',#14,.T.);\n#16=FACE('',(#15));\n\
             #17=POLY_LOOP('',(#1,#4,#3));\n#18=FACE_OUTER_BOUND('',#17,.T.);\n#19=FACE('',(#18));\n\
             #20=POLY_LOOP('',(#2,#3,#4));\n#21=FACE_OUTER_BOUND('',#20,.T.);\n#22=FACE('',(#21));\n\
             #30=CLOSED_SHELL('',(#13,#16,#19,#22));\n#31=FACETED_BREP('',#30);",
            31,
        );
        let body = only_body(&text);
        assert_eq!((body.vertices.len(), body.edges.len(), body.faces.len()), (4, 6, 4));
        assert_near(&body, 1.0 / 6.0, "tetrahedron");
    }

    #[test]
    fn a_cone_ends_at_its_apex_vertex_loop() {
        let text = solid_file(
            "#1=CARTESIAN_POINT('',(0.,0.,0.));\n\
             #2=DIRECTION('',(0.,0.,1.));\n\
             #3=DIRECTION('',(1.,0.,0.));\n\
             #4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n\
             #5=CIRCLE('',#4,1.);\n\
             #6=CARTESIAN_POINT('',(1.,0.,0.));\n\
             #7=VERTEX_POINT('',#6);\n\
             #8=EDGE_CURVE('',#7,#7,#5,.T.);\n\
             #9=ORIENTED_EDGE('',*,*,#8,.F.);\n\
             #10=EDGE_LOOP('',(#9));\n\
             #11=FACE_OUTER_BOUND('',#10,.T.);\n\
             #12=PLANE('',#4);\n\
             #13=ADVANCED_FACE('',(#11),#12,.F.);\n\
             #20=CARTESIAN_POINT('',(0.,0.,1.));\n\
             #21=DIRECTION('',(0.,0.,-1.));\n\
             #22=AXIS2_PLACEMENT_3D('',#20,#21,#3);\n\
             #23=CONICAL_SURFACE('',#22,0.,0.785398163397448);\n\
             #24=VERTEX_POINT('',#20);\n\
             #25=VERTEX_LOOP('',#24);\n\
             #26=FACE_BOUND('',#25,.T.);\n\
             #27=ORIENTED_EDGE('',*,*,#8,.T.);\n\
             #28=EDGE_LOOP('',(#27));\n\
             #29=FACE_OUTER_BOUND('',#28,.T.);\n\
             #30=ADVANCED_FACE('',(#29,#26),#23,.T.);\n\
             #31=CLOSED_SHELL('',(#13,#30));\n\
             #32=MANIFOLD_SOLID_BREP('',#31);",
            32,
        );
        assert_near(&only_body(&text), std::f64::consts::PI / 3.0, "cone");
    }

    #[test]
    fn faces_that_share_no_edges_are_sewn_into_a_solid() {
        // A 10 mm cube whose six faces each name their own corners and
        // edges, every other face 10 µm off: more than the file's 1 nm
        // uncertainty, so the tolerance has to grow.
        let corner = |i: usize, shift: f64| {
            [(i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64]
                .map(|v| v * 10.0 + shift)
        };
        let faces: [([usize; 4], [f64; 3]); 6] = [
            ([0, 2, 3, 1], [0.0, 0.0, -1.0]),
            ([4, 5, 7, 6], [0.0, 0.0, 1.0]),
            ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
            ([2, 6, 7, 3], [0.0, 1.0, 0.0]),
            ([0, 4, 6, 2], [-1.0, 0.0, 0.0]),
            ([1, 3, 7, 5], [1.0, 0.0, 0.0]),
        ];
        let mut data = String::new();
        let mut next = 1;
        let mut id = || {
            next += 1;
            next - 1
        };
        let mut face_ids = Vec::new();
        for (k, (corners, normal)) in faces.iter().enumerate() {
            let shift = if k % 2 == 0 { 0.0 } else { 1e-5 };
            let points: Vec<[f64; 3]> = corners.iter().map(|&c| corner(c, shift)).collect();
            let mut vertices = Vec::new();
            for p in &points {
                let (cp, vp) = (id(), id());
                data += &format!("#{cp}=CARTESIAN_POINT('',({:?},{:?},{:?}));\n", p[0], p[1], p[2]);
                data += &format!("#{vp}=VERTEX_POINT('',#{cp});\n");
                vertices.push((cp, vp));
            }
            let mut oriented = Vec::new();
            for i in 0..4 {
                let (a, b) = (points[i], points[(i + 1) % 4]);
                let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let (dir, vec, line, edge, use_) = (id(), id(), id(), id(), id());
                data += &format!("#{dir}=DIRECTION('',({:?},{:?},{:?}));\n", d[0], d[1], d[2]);
                data += &format!("#{vec}=VECTOR('',#{dir},1.);\n");
                data += &format!("#{line}=LINE('',#{},#{vec});\n", vertices[i].0);
                data += &format!(
                    "#{edge}=EDGE_CURVE('',#{},#{},#{line},.T.);\n",
                    vertices[i].1,
                    vertices[(i + 1) % 4].1
                );
                data += &format!("#{use_}=ORIENTED_EDGE('',*,*,#{edge},.T.);\n");
                oriented.push(format!("#{use_}"));
            }
            let (ring, bound, axis, reference, placement, plane, face) =
                (id(), id(), id(), id(), id(), id(), id());
            data += &format!("#{ring}=EDGE_LOOP('',({}));\n", oriented.join(","));
            data += &format!("#{bound}=FACE_OUTER_BOUND('',#{ring},.T.);\n");
            data += &format!(
                "#{axis}=DIRECTION('',({:?},{:?},{:?}));\n",
                normal[0], normal[1], normal[2]
            );
            let x = if normal[0] == 0.0 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            data += &format!("#{reference}=DIRECTION('',({:?},{:?},{:?}));\n", x[0], x[1], x[2]);
            data += &format!(
                "#{placement}=AXIS2_PLACEMENT_3D('',#{},#{axis},#{reference});\n",
                vertices[0].0
            );
            data += &format!("#{plane}=PLANE('',#{placement});\n");
            data += &format!("#{face}=ADVANCED_FACE('',(#{bound}),#{plane},.T.);\n");
            face_ids.push(format!("#{face}"));
        }
        let (shell, solid) = (id(), id());
        data += &format!("#{shell}=CLOSED_SHELL('',({}));\n", face_ids.join(","));
        data += &format!("#{solid}=MANIFOLD_SOLID_BREP('',#{shell});");
        let body = only_body(&solid_file(&data, solid));
        assert_eq!((body.vertices.len(), body.edges.len()), (8, 12));
        assert!(ocs_solid::model::is_solid(&body), "sewn closed");
        assert_near(&body, 1000.0, "sewn cube");
    }

    #[test]
    fn the_uncertainty_is_read_in_its_own_unit() {
        let doc = doc("#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.));\n\
             #2=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
             #3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.01),#2,'distance_accuracy_value','');\n\
             #4=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#3)) \
             GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('',''));");
        // 0.01 mm in a file measured in metres.
        let tolerance = uncertainty(&doc, 4, 1000.0).unwrap();
        assert!((tolerance - 1e-5).abs() < 1e-18, "{tolerance}");
    }

    /// A seeded panic fuzz (after solidcore's `solidcore-step` panic fuzz):
    /// files mutated at random (truncated, bytes flipped, numbers made huge,
    /// NaN or infinite, references broken or turned back on themselves,
    /// lines dropped or repeated, entity names swapped, lists nested deep)
    /// are read; reading nothing is fine, a panic fails with its seed.
    #[test]
    fn mutated_files_never_panic() {
        const NUMBERS: &[&str] = &[
            "NAN", "1.E999", "-1.E999", "1.E308", "1.E-320", "0.", "-0.", "-1", "0",
            "4294967296", "18446744073709551616", "9223372036854775807", "99999999999", ".5",
            "1E", "--1", "33", "1000000",
        ];
        const REFERENCES: &[&str] = &["0", "1", "2", "99999999", "18446744073709551616", "", "#"];
        const ENTITIES: &[&str] = &[
            "CARTESIAN_POINT", "DIRECTION", "VECTOR", "LINE", "CIRCLE", "ELLIPSE", "PARABOLA",
            "HYPERBOLA", "PLANE", "CYLINDRICAL_SURFACE", "CONICAL_SURFACE", "TOROIDAL_SURFACE",
            "B_SPLINE_CURVE_WITH_KNOTS", "B_SPLINE_SURFACE_WITH_KNOTS", "BEZIER_CURVE",
            "UNIFORM_SURFACE", "SURFACE_OF_REVOLUTION", "SURFACE_OF_LINEAR_EXTRUSION",
            "OFFSET_SURFACE", "RECTANGULAR_TRIMMED_SURFACE", "PCURVE", "SURFACE_CURVE",
            "EDGE_CURVE", "VERTEX_LOOP", "POLY_LOOP", "EDGE_LOOP", "ORIENTED_FACE",
            "ADVANCED_FACE", "FACE", "CLOSED_SHELL", "FACETED_BREP", "BREP_WITH_VOIDS",
            "MAPPED_ITEM", "NEXT_ASSEMBLY_USAGE_OCCURRENCE",
        ];
        let surfaces = cylinder_side_as(
            "SURFACE_OF_REVOLUTION('',#9001,#9006)",
            "#9000=CARTESIAN_POINT('',(5.,0.,0.));\n#9002=DIRECTION('',(0.,0.,1.));\n\
             #9003=VECTOR('',#9002,1.);\n#9001=LINE('',#9000,#9003);\n\
             #9005=CARTESIAN_POINT('',(0.,0.,0.));\n#9006=AXIS1_PLACEMENT('',#9005,#9002);",
        );
        let bases: Vec<String> = [BOX, CURVED, NURBS_SPHERE, ASSEMBLY, OPEN_BOX, NO_PRODUCTS]
            .iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .chain([surfaces])
            .collect();
        let cases: u64 = std::env::var("OCS_STEP_FUZZ_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(400);
        // xorshift64*: a fixed seed per case, so a failure names its case.
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = |n: usize| -> usize {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % n.max(1)
        };
        let token_at = |text: &str, from: usize, test: fn(char) -> bool| {
            let from = (from..text.len()).find(|&i| text.is_char_boundary(i))?;
            let start = text[from..].find(test)? + from;
            let end = text[start..].find(|c: char| !test(c)).map_or(text.len(), |e| e + start);
            Some((start, end))
        };
        for case in 0..cases {
            let mut text = bases[next(bases.len())].clone();
            let mut edits = Vec::new();
            for _ in 0..1 + next(3) {
                let at = next(text.len().max(1));
                let edit = next(9);
                edits.push((edit, at));
                match edit {
                    0 => {
                        let cut = (at..=text.len()).find(|&i| text.is_char_boundary(i)).unwrap();
                        text.truncate(cut);
                    }
                    1 => {
                        let mut bytes = text.into_bytes();
                        if let Some(b) = bytes.get_mut(at) {
                            *b ^= 1 << next(7);
                        }
                        text = String::from_utf8_lossy(&bytes).into_owned();
                    }
                    2 => {
                        let number = |c: char| c.is_ascii_digit() || c == '.';
                        if let Some((s, e)) = token_at(&text, at, number) {
                            text.replace_range(s..e, NUMBERS[next(NUMBERS.len())]);
                        }
                    }
                    3 => {
                        if let Some(s) = text.get(at..).and_then(|t| t.find('#')).map(|s| s + at) {
                            let end = text[s + 1..]
                                .find(|c: char| !c.is_ascii_digit())
                                .map_or(text.len(), |e| e + s + 1);
                            text.replace_range(s + 1..end, REFERENCES[next(REFERENCES.len())]);
                        }
                    }
                    4 | 5 => {
                        let lines: Vec<&str> = text.lines().collect();
                        let n = next(lines.len());
                        let mut out: Vec<&str> = lines.clone();
                        if edit == 4 {
                            out.remove(n.min(out.len().saturating_sub(1)));
                        } else if let Some(line) = lines.get(n) {
                            for _ in 0..1 + next(3) {
                                out.insert(n, line);
                            }
                        }
                        text = out.join("\n");
                    }
                    6 => {
                        let upper = |c: char| c.is_ascii_uppercase() || c == '_';
                        if let Some((s, e)) = token_at(&text, at, upper) {
                            text.replace_range(s..e, ENTITIES[next(ENTITIES.len())]);
                        }
                    }
                    7 => {
                        let cut = (at..=text.len()).find(|&i| text.is_char_boundary(i)).unwrap();
                        let depth = [3, 70, 5000][next(3)];
                        let nest = format!("{}{}", "(".repeat(depth), ")".repeat(depth));
                        text.insert_str(cut, &nest);
                    }
                    _ => {
                        // A reference made to point back at its own instance.
                        if let Some(s) = text.get(at..).and_then(|t| t.find("\n#")) {
                            let line = at + s + 1;
                            let id_end = text[line + 1..]
                                .find(|c: char| !c.is_ascii_digit())
                                .map_or(text.len(), |e| e + line + 1);
                            let id = text[line + 1..id_end].to_string();
                            if let Some(r) = text[id_end..].find('#') {
                                let r = r + id_end;
                                let r_end = text[r + 1..]
                                    .find(|c: char| !c.is_ascii_digit())
                                    .map_or(text.len(), |e| e + r + 1);
                                text.replace_range(r + 1..r_end, &id);
                            }
                        }
                    }
                }
            }
            let outcome = std::panic::catch_unwind(|| read(text.as_bytes()).is_some());
            assert!(outcome.is_ok(), "case {case} panicked; edits {edits:?}");
        }
    }
}
