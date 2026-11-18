//! ISO 10303-21 ("Part 21") exchange structure parser: the HEADER and the DATA section's entity
//! instances, simple (`#1=NAME(...)`) and complex (`#2=(A(...) B(...))`), with the string escapes
//! decoded (`\X\hh`, `\X2\…\X0\`, `\X4\…\X0\`, `\S\c`, `\\`). Written from the public ISO 10303-21
//! specification.
//!
//! Input is hostile: the text size ([`MAX_BYTES`]), the entity count ([`MAX_ENTITIES`]), the record
//! count ([`MAX_RECORDS`]), the parameter count ([`MAX_PARAMS`]), the parameter nesting depth
//! (`MAX_DEPTH`, private) and the parts of a complex instance (`MAX_COMPLEX_PARTS`, private) are
//! capped, nothing is allocated from a count read out of the file (lists grow as their items are
//! read), and every refusal is an [`Error`], never a panic.
//!
//! Origin: SolidCraft's `crates/step/src/p21.rs` at `a5b71ca2` (MIT OR Apache-2.0). This is
//! solidcore's one Part 21 implementation (`step-import`'s `part21.rs` is gone, #14123).
//!
//! Deviations from SolidCraft: [`Exchange::entities`] is a `BTreeMap` (was a `HashMap`), so
//! iterating it is in id order on every platform; what is stored and which definition wins a
//! duplicate id are unchanged. The complex-instance part limit is the named `MAX_COMPLEX_PARTS`
//! (was the literal 64). Otherwise only lint-driven rewrites that keep the same result (`map_or`,
//! `u32::from`, or-patterns, the error line number counted as the newline-split pieces) and doc
//! comments on every public item.
//!
//! Changed in review (#2591): a parse refuses with a typed [`Error`] (the messages are
//! SolidCraft's); unterminated `\X2\`/`\X4\` runs decode in linear time; the parameter count is
//! capped ([`MAX_PARAMS`]), so is the record count ([`MAX_RECORDS`]), and a repeated id counts
//! against [`MAX_ENTITIES`]; a number with no digit and an integer beyond i64 are refused; a
//! leading byte order mark is skipped; `\S\` before a UTF-8 character is kept as written.
//!
//! Limit: an ANCHOR, REFERENCE or SIGNATURE section (edition 3) is skipped to the first `ENDSEC;`
//! in the raw text, so one inside a string in such a section ends the skip early and the rest is
//! read as sections. No corpus file has these sections.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::num::IntErrorKind;

/// Largest STEP text [`parse`] and [`parse_with`] read. A caller whose user raised the limit
/// passes its own to [`parse_with_max_bytes`] (SolidCraft's Preferences › General › Largest file
/// to open, #13329).
pub const MAX_BYTES: usize = 512 << 20;
/// Most entity instances in one file, a repeated id counted each time.
pub const MAX_ENTITIES: usize = 20_000_000;
/// Deepest parameter list nesting.
const MAX_DEPTH: usize = 64;
/// A complex instance is refused when more than this many of its parts have been read and
/// another follows.
const MAX_COMPLEX_PARTS: usize = 64;
/// Most records in one file's DATA sections, each part of a complex instance counted.
///
/// A record holds no parameter when its list is empty, so [`MAX_PARAMS`] does not bound them:
/// `A()` is three bytes and its [`Record`] 48 (on a 64-bit target) plus its name, so a text at
/// the size cap of empty complex-instance parts could ask for about 8 GB of record slots. This
/// cap holds them to under 1 GB there; the whole corpus holds under a million instances.
pub const MAX_RECORDS: usize = 20_000_000;
/// Most parameters in one file, nested ones and the header's included.
///
/// [`MAX_BYTES`] alone does not bound memory: `$,` is two bytes and its [`Param`] 48 (on a
/// 64-bit target), so a text at the size cap could ask for about 13 GB. This cap holds the
/// parameters themselves to under 1 GB there, and less on wasm32, where a `Param` is smaller.
/// The densest corpus file (NIST `nist_stc_09_asme1_ap242-e3.stp`, about 5.4 bytes a parameter
/// counting its separators) would reach it at about 110 MB; it holds under a million.
pub const MAX_PARAMS: usize = 20_000_000;

/// The text size and the counts a parse refuses beyond: [`MAX_BYTES`], [`MAX_ENTITIES`],
/// [`MAX_RECORDS`] and [`MAX_PARAMS`] by default, all raised together when a caller raises the
/// size ([`Limits::for_bytes`]), smaller in tests so a limit is reached with a short text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Limits {
    bytes: usize,
    entities: usize,
    records: usize,
    params: usize,
}

const LIMITS: Limits = Limits {
    bytes: MAX_BYTES,
    entities: MAX_ENTITIES,
    records: MAX_RECORDS,
    params: MAX_PARAMS,
};

impl Limits {
    /// The limits for a text of up to `max_bytes` (never below the defaults). The counts grow in
    /// proportion to the size, so a larger file of the same density as one the defaults read
    /// is read too, and the memory they bound stays the same multiple of the text's size.
    fn for_bytes(max_bytes: usize) -> Self {
        let bytes = max_bytes.max(MAX_BYTES);
        let scale = |count: usize| {
            let scaled = count as u128 * bytes as u128 / MAX_BYTES as u128;
            usize::try_from(scaled).unwrap_or(usize::MAX)
        };
        Self {
            bytes,
            entities: scale(MAX_ENTITIES),
            records: scale(MAX_RECORDS),
            params: scale(MAX_PARAMS),
        }
    }
}

/// A parameter value.
#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    /// An integer.
    Int(i64),
    /// A real (`1.`, `1.5E1`, `1.E-3`).
    Real(f64),
    /// A string, its escapes decoded.
    Str(String),
    /// `.NAME.` (`.T.`/`.F.` included).
    Enum(String),
    /// An entity instance name, `#n`.
    Ref(u64),
    /// `(a, b, ...)`.
    List(Vec<Param>),
    /// `NAME(params)` (a typed value such as `LENGTH_MEASURE(1.)`).
    Typed(String, Vec<Param>),
    /// `"hex"`, the text between the quotes as written.
    Binary(String),
    /// `$`
    Null,
    /// `*`
    Derived,
}

impl Param {
    /// A finite real, an integer as a real, or a typed value holding one of them.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Param::Real(x) => Some(*x).filter(|x| x.is_finite()),
            Param::Int(i) => Some(*i as f64),
            Param::Typed(_, v) if v.len() == 1 => v.first().and_then(Param::as_f64),
            _ => None,
        }
    }
    /// An integer, or a real with no fraction below `9e15` in magnitude.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Param::Int(i) => Some(*i),
            Param::Real(x) if x.fract() == 0.0 && x.abs() < 9e15 => Some(*x as i64),
            _ => None,
        }
    }
    /// The id of an entity reference.
    pub fn as_ref_id(&self) -> Option<u64> {
        match self {
            Param::Ref(r) => Some(*r),
            _ => None,
        }
    }
    /// The text of a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Param::Str(s) => Some(s),
            _ => None,
        }
    }
    /// The name of an enumeration, without its dots.
    pub fn as_enum(&self) -> Option<&str> {
        match self {
            Param::Enum(s) => Some(s),
            _ => None,
        }
    }
    /// `.T.` → true, `.F.` → false (`.U.` and anything else → None).
    pub fn as_bool(&self) -> Option<bool> {
        match self.as_enum()? {
            "T" => Some(true),
            "F" => Some(false),
            _ => None,
        }
    }
    /// The items of a list.
    pub fn as_list(&self) -> Option<&[Param]> {
        match self {
            Param::List(v) => Some(v),
            _ => None,
        }
    }
}

/// One record of an entity instance: a simple instance has one, a complex one several.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// The entity type name, upper case.
    pub name: String,
    /// The record's parameters, in file order.
    pub params: Vec<Param>,
}

/// An entity instance.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    /// Its records, in file order.
    pub records: Vec<Record>,
}

impl Entity {
    /// Whether the instance has more than one record.
    pub fn is_complex(&self) -> bool {
        self.records.len() > 1
    }
    /// Name of a simple instance (the first record of a complex one).
    pub fn name(&self) -> &str {
        self.records.first().map_or("", |r| r.name.as_str())
    }
    /// Parameters of the record called `name`.
    pub fn record(&self, name: &str) -> Option<&[Param]> {
        self.records
            .iter()
            .find(|r| r.name == name)
            .map(|r| r.params.as_slice())
    }
    /// Whether the instance has a record called `name`.
    pub fn has(&self, name: &str) -> bool {
        self.records.iter().any(|r| r.name == name)
    }
    /// Parameters of a simple instance.
    pub fn params(&self) -> &[Param] {
        self.records.first().map_or(&[], |r| r.params.as_slice())
    }
}

/// The HEADER section's file description and file name, as written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// `FILE_DESCRIPTION` lines.
    pub descriptions: Vec<String>,
    /// `FILE_DESCRIPTION` implementation level.
    pub implementation_level: String,
    /// `FILE_NAME` name.
    pub file_name: String,
    /// `FILE_NAME` time stamp.
    pub time_stamp: String,
    /// `FILE_NAME` authors.
    pub authors: Vec<String>,
    /// `FILE_NAME` organizations.
    pub organizations: Vec<String>,
    /// `FILE_NAME` preprocessor version.
    pub preprocessor_version: String,
    /// `FILE_NAME` originating system.
    pub originating_system: String,
    /// `FILE_NAME` authorization.
    pub authorization: String,
}

/// A parsed exchange file.
#[derive(Clone, Debug, Default)]
pub struct Exchange {
    /// The HEADER's description and file name, field by field.
    pub header: Header,
    /// `FILE_SCHEMA` names.
    pub schemas: Vec<String>,
    /// `FILE_NAME` originating system.
    pub originating_system: String,
    /// `FILE_NAME` preprocessor version.
    pub preprocessor: String,
    /// `FILE_DESCRIPTION` text, its lines joined by a space.
    pub description: String,
    /// The DATA sections' entity instances by id.
    pub entities: BTreeMap<u64, Entity>,
    /// Ids defined more than once (the first definition is kept).
    pub duplicates: Vec<u64>,
}

impl Exchange {
    /// The entity instance `#id`.
    pub fn get(&self, id: u64) -> Option<&Entity> {
        self.entities.get(&id)
    }
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
    /// Parameters read so far, header and data.
    params: usize,
    /// The most parameters to read.
    max_params: usize,
    /// DATA records read so far.
    records: usize,
    /// The most records to read.
    max_records: usize,
    /// The last place `line` counted to and the newlines before it, so errors met one after
    /// another while skipping bad header records count each byte once.
    counted: Cell<(usize, usize)>,
}

/// Why a text was refused. Its message is the one `parse` returned as a string before it had
/// a type; `String::from` gives it back for callers that collect messages.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The text is longer than the limit ([`MAX_BYTES`] unless the caller raised it).
    #[error("STEP file too large ({} MB, limit {} MB)", .bytes >> 20, .limit >> 20)]
    TooLarge {
        /// The text's length.
        bytes: usize,
        /// The limit it is longer than, in bytes.
        limit: usize,
    },
    /// The text does not start with `ISO-10303-21;`.
    #[error("not a STEP file (no ISO-10303-21 header)")]
    NotStep,
    /// The text ends before `END-ISO-10303-21;`.
    #[error("STEP file ends without END-ISO-10303-21")]
    MissingEnd,
    /// More entity instances than the limit.
    #[error("STEP file has more than {limit} entities")]
    TooManyEntities {
        /// The limit, [`MAX_ENTITIES`].
        limit: usize,
    },
    /// More records than the limit.
    #[error("STEP file has more than {limit} records")]
    TooManyRecords {
        /// The limit, [`MAX_RECORDS`].
        limit: usize,
    },
    /// More parameters than the limit.
    #[error("STEP file has more than {limit} parameters")]
    TooManyParams {
        /// The limit, [`MAX_PARAMS`].
        limit: usize,
    },
    /// The [`Progress`] watcher cancelled.
    #[error("STEP import cancelled")]
    Cancelled,
    /// The text breaks the exchange structure's syntax.
    #[error("STEP syntax error at line {line}: {message}")]
    Syntax {
        /// The line, counted from 1.
        line: usize,
        /// What was wrong.
        message: String,
    },
}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

impl Lexer<'_> {
    fn line(&self) -> usize {
        let (at, before) = self.counted.get();
        let (from, before) = if self.i >= at { (at, before) } else { (0, 0) };
        let Some(b) = self.s.get(from..self.i) else {
            return 0;
        };
        #[allow(clippy::naive_bytecount)] // no bytecount dependency in the record
        let newlines = before + b.iter().filter(|c| **c == b'\n').count();
        self.counted.set((self.i, newlines));
        newlines + 1
    }
    /// Where the next line after `from` that starts with a keyword begins.
    fn next_header_line(&self, from: usize) -> Option<usize> {
        let rest = self.s.get(from..)?;
        let mut at = rest.iter().position(|&c| c == b'\n')? + 1;
        loop {
            let line = at
                + rest
                    .get(at..)?
                    .iter()
                    .take_while(|c| matches!(c, b' ' | b'\t' | b'\r'))
                    .count();
            if rest.get(line)?.is_ascii_uppercase() {
                return Some(from + line);
            }
            at += rest.get(at..)?.iter().position(|&c| c == b'\n')? + 1;
        }
    }
    fn fail<T>(&self, msg: &str) -> Result<T, Error> {
        Err(Error::Syntax {
            line: self.line(),
            message: msg.to_string(),
        })
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    /// Skip whitespace and comments.
    fn ws(&mut self) -> Result<(), Error> {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => self.i += 1,
                Some(b'/') if self.s.get(self.i + 1) == Some(&b'*') => {
                    let rest = self.s.get(self.i + 2..).unwrap_or(&[]);
                    match rest.windows(2).position(|w| w == b"*/") {
                        Some(p) => self.i += p + 4,
                        None => return self.fail("unterminated comment"),
                    }
                }
                _ => return Ok(()),
            }
        }
    }
    fn eat(&mut self, c: u8) -> Result<bool, Error> {
        self.ws()?;
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn expect(&mut self, c: u8) -> Result<(), Error> {
        if self.eat(c)? {
            Ok(())
        } else {
            self.fail(&format!("expected `{}`", c as char))
        }
    }
    fn keyword(&mut self) -> Result<String, Error> {
        self.ws()?;
        let st = self.i;
        if self.peek() == Some(b'!') {
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' {
                self.i += 1;
            } else {
                break;
            }
        }
        if self.i == st {
            return self.fail("expected a keyword");
        }
        Ok(String::from_utf8_lossy(self.s.get(st..self.i).unwrap_or(&[])).to_ascii_uppercase())
    }
    fn number(&mut self) -> Result<Param, Error> {
        let st = self.i;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.' | b'E' | b'e') {
                self.i += 1;
            } else {
                break;
            }
        }
        let t = std::str::from_utf8(self.s.get(st..self.i).unwrap_or(&[])).unwrap_or("");
        // Part 21 numbers start with a digit after the sign; a lone `.` or `+.` (or `-.E3`)
        // would otherwise become 0.0 through the `1.` fix-up below.
        let mantissa = t.split(['E', 'e']).next().unwrap_or("");
        if !mantissa.bytes().any(|c| c.is_ascii_digit()) {
            return self.fail(&format!("bad number `{t}`"));
        }
        if !t.contains(['.', 'E', 'e']) {
            return match t.parse::<i64>() {
                Ok(i) => Ok(Param::Int(i)),
                // An integer is a count or an index: one beyond i64 is refused, not rounded
                // into a real.
                Err(e)
                    if matches!(
                        e.kind(),
                        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow
                    ) =>
                {
                    self.fail(&format!("integer `{t}` out of range"))
                }
                Err(_) => self.fail(&format!("bad number `{t}`")),
            };
        }
        // STEP allows `1.` and `1.E-3`.
        let fixed = t.replace(".E", ".0E").replace(".e", ".0e");
        let fixed = if fixed.ends_with('.') {
            format!("{fixed}0")
        } else {
            fixed
        };
        match fixed.parse::<f64>() {
            Ok(x) => Ok(Param::Real(x)),
            Err(_) => self.fail(&format!("bad number `{t}`")),
        }
    }
    fn string(&mut self) -> Result<String, Error> {
        // Opening quote consumed by the caller.
        let mut raw = Vec::new();
        loop {
            match self.peek() {
                None => return self.fail("unterminated string"),
                Some(b'\'') => {
                    if self.s.get(self.i + 1) == Some(&b'\'') {
                        raw.push(b'\'');
                        self.i += 2;
                    } else {
                        self.i += 1;
                        break;
                    }
                }
                Some(b'\n' | b'\r') => self.i += 1,
                Some(c) => {
                    raw.push(c);
                    self.i += 1;
                }
            }
        }
        Ok(decode_string(&raw))
    }
    fn param(&mut self, depth: usize) -> Result<Param, Error> {
        if depth > MAX_DEPTH {
            return self.fail("parameters nested too deeply");
        }
        if self.params >= self.max_params {
            return Err(Error::TooManyParams {
                limit: self.max_params,
            });
        }
        self.params += 1;
        self.ws()?;
        let Some(c) = self.peek() else {
            return self.fail("unexpected end of file");
        };
        match c {
            b'$' => {
                self.i += 1;
                Ok(Param::Null)
            }
            b'*' => {
                self.i += 1;
                Ok(Param::Derived)
            }
            b'\'' => {
                self.i += 1;
                Ok(Param::Str(self.string()?))
            }
            b'"' => {
                self.i += 1;
                let st = self.i;
                while self.peek().is_some_and(|c| c != b'"') {
                    self.i += 1;
                }
                let v = String::from_utf8_lossy(self.s.get(st..self.i).unwrap_or(&[])).to_string();
                self.expect(b'"')?;
                Ok(Param::Binary(v))
            }
            b'#' => {
                self.i += 1;
                let st = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
                let t = std::str::from_utf8(self.s.get(st..self.i).unwrap_or(&[])).unwrap_or("");
                t.parse::<u64>()
                    .map(Param::Ref)
                    .or_else(|_| self.fail("bad entity reference"))
            }
            b'.' if self
                .s
                .get(self.i + 1)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') =>
            {
                self.i += 1;
                let st = self.i;
                while self.peek().is_some_and(|c| c != b'.') {
                    self.i += 1;
                }
                let v = String::from_utf8_lossy(self.s.get(st..self.i).unwrap_or(&[]))
                    .trim()
                    .to_ascii_uppercase();
                self.expect(b'.')?;
                Ok(Param::Enum(v))
            }
            b'(' => {
                self.i += 1;
                Ok(Param::List(self.list_rest(depth + 1)?))
            }
            c if c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.') => self.number(),
            c if c.is_ascii_alphabetic() || c == b'!' => {
                let name = self.keyword()?;
                self.expect(b'(')?;
                Ok(Param::Typed(name, self.list_rest(depth + 1)?))
            }
            _ => self.fail(&format!("unexpected `{}`", c as char)),
        }
    }
    /// Parameters after an opening `(` up to and including the `)`.
    fn list_rest(&mut self, depth: usize) -> Result<Vec<Param>, Error> {
        let mut v = Vec::new();
        if self.eat(b')')? {
            return Ok(v);
        }
        loop {
            v.push(self.param(depth)?);
            if self.eat(b',')? {
                continue;
            }
            self.expect(b')')?;
            return Ok(v);
        }
    }
    fn record(&mut self) -> Result<Record, Error> {
        if self.records >= self.max_records {
            return Err(Error::TooManyRecords {
                limit: self.max_records,
            });
        }
        self.records += 1;
        let name = long_name(self.keyword()?);
        self.expect(b'(')?;
        Ok(Record {
            name,
            params: self.list_rest(1)?,
        })
    }
}

/// Part 21 short entity names (the short-name form ISO 10303-21 allows, which some AP203 writers
/// use, STEP Tools' among them) of the entities a B-rep, its representation, product and units
/// are read from, with their long names. Every pair is as ISO's short-names file gives it
/// (<http://standards.iso.org/iso/10303/tech/short_names/short-names.txt>), checked entry by entry.
/// `step-import`'s table (#2582) mapped `CMCRSG` to `COMPOSITE_CURVE`; it is
/// `COMPOSITE_CURVE_SEGMENT`, and `COMPOSITE_CURVE` is `CMPCRV`.
const SHORT_NAMES: [(&str, &str); 36] = [
    ("A2PL3D", "AXIS2_PLACEMENT_3D"),
    ("ADVFC", "ADVANCED_FACE"),
    ("BSCWK", "B_SPLINE_CURVE_WITH_KNOTS"),
    ("BSSWK", "B_SPLINE_SURFACE_WITH_KNOTS"),
    ("CLSSHL", "CLOSED_SHELL"),
    ("CMCRSG", "COMPOSITE_CURVE_SEGMENT"),
    ("CMPCRV", "COMPOSITE_CURVE"),
    ("CNBSUN", "CONVERSION_BASED_UNIT"),
    ("CRTPNT", "CARTESIAN_POINT"),
    ("DMNEXP", "DIMENSIONAL_EXPONENTS"),
    ("DRCTN", "DIRECTION"),
    ("EDGCRV", "EDGE_CURVE"),
    ("EDGLP", "EDGE_LOOP"),
    ("FBSR", "FACETED_BREP_SHAPE_REPRESENTATION"),
    ("FCBND", "FACE_BOUND"),
    ("FCOTBN", "FACE_OUTER_BOUND"),
    ("FCSRF", "FACE_SURFACE"),
    ("FCTBR", "FACETED_BREP"),
    ("GMRPCN", "GEOMETRIC_REPRESENTATION_CONTEXT"),
    ("GMTST", "GEOMETRIC_SET"),
    ("LMWU", "LENGTH_MEASURE_WITH_UNIT"),
    ("LNGUNT", "LENGTH_UNIT"),
    ("MNSLBR", "MANIFOLD_SOLID_BREP"),
    ("NMDUNT", "NAMED_UNIT"),
    ("ORNEDG", "ORIENTED_EDGE"),
    ("PAMWU", "PLANE_ANGLE_MEASURE_WITH_UNIT"),
    ("PLYLP", "POLY_LOOP"),
    ("PRDCT", "PRODUCT"),
    ("PRDDFN", "PRODUCT_DEFINITION"),
    ("PRDFSH", "PRODUCT_DEFINITION_SHAPE"),
    ("SHDFRP", "SHAPE_DEFINITION_REPRESENTATION"),
    ("SHPRPR", "SHAPE_REPRESENTATION"),
    ("SHRPRL", "SHAPE_REPRESENTATION_RELATIONSHIP"),
    ("TRMCRV", "TRIMMED_CURVE"),
    ("UMWU", "UNCERTAINTY_MEASURE_WITH_UNIT"),
    ("VRTPNT", "VERTEX_POINT"),
];

/// An entity name as its long form where it is one of [`SHORT_NAMES`].
fn long_name(name: String) -> String {
    match SHORT_NAMES.iter().find(|(short, _)| *short == name) {
        Some((_, long)) => (*long).to_string(),
        None => name,
    }
}

/// Decode the Part 21 string escapes (`\X\hh`, `\X2\…\X0\`, `\X4\…\X0\`, `\S\c`, `\\`).
fn decode_string(raw: &[u8]) -> String {
    decode_string_with(raw, |rest| rest.windows(4).position(|w| w == b"\\X0\\"))
}

/// [`decode_string`] with the search for a run's `\X0\` given, so a test can count its steps.
/// `find_end(rest)` is the offset of the first `\X0\` in `rest`.
///
/// A `\X2\` or `\X4\` with no `\X0\` after it is kept as written. The search for the
/// terminator is remembered: the first `\X0\` at or after one position is also the first at or
/// after every later position up to it, and when none follows one position none follows any
/// later one. So a search starts only past the terminator the last one found, the searches
/// cover disjoint stretches of `raw`, and decoding is linear however many unterminated runs the
/// string holds (searching again from each one made it quadratic).
fn decode_string_with(raw: &[u8], mut find_end: impl FnMut(&[u8]) -> Option<usize>) -> String {
    let mut out = String::new();
    let mut i = 0;
    // The last search's answer: the position in `raw` of the first `\X0\` it found (`None`:
    // none to the end). `None` outside: no search yet.
    let mut next_end: Option<Option<usize>> = None;
    let hex = |b: &[u8]| {
        std::str::from_utf8(b)
            .ok()
            .and_then(|t| u32::from_str_radix(t, 16).ok())
    };
    while let Some(&c) = raw.get(i) {
        if c == b'\\' {
            let rest = raw.get(i..).unwrap_or(&[]);
            if rest.starts_with(b"\\\\") {
                out.push('\\');
                i += 2;
                continue;
            }
            if rest.starts_with(b"\\X\\")
                && let Some(v) = rest.get(3..5).and_then(hex)
            {
                out.push(char::from_u32(v).unwrap_or('?'));
                i += 5;
                continue;
            }
            let mut run = false;
            for (tag, width) in [(&b"\\X2\\"[..], 4usize), (&b"\\X4\\"[..], 8)] {
                if !rest.starts_with(tag) {
                    continue;
                }
                let found = match next_end {
                    Some(Some(end)) if end >= i => Some(end),
                    Some(None) => None,
                    _ => {
                        let found = find_end(rest).map(|end| i + end);
                        next_end = Some(found);
                        found
                    }
                };
                if let Some(end) = found.and_then(|end| end.checked_sub(i)) {
                    let body = rest.get(4..end).unwrap_or(&[]);
                    let units: Vec<u32> = body.chunks(width).filter_map(hex).collect();
                    if width == 4 {
                        let u16s: Vec<u16> = units.iter().map(|u| *u as u16).collect();
                        out.push_str(&String::from_utf16_lossy(&u16s));
                    } else {
                        out.extend(units.iter().map(|u| char::from_u32(*u).unwrap_or('?')));
                    }
                    i += end + 4;
                    run = true;
                    break;
                }
            }
            // A run may be followed straight away by the backslash of the next escape
            // (SolidCraft #2344).
            if run {
                continue;
            }
            // `\S\` takes a basic-alphabet character (space to `~`); before any other byte,
            // the start of a UTF-8 character, adding 128 would split it into mojibake, so the
            // escape is kept as written.
            if rest.starts_with(b"\\S\\")
                && let Some(&ch) = rest.get(3)
                && (b' '..=b'~').contains(&ch)
            {
                out.push(char::from_u32(u32::from(ch) + 128).unwrap_or('?'));
                i += 4;
                continue;
            }
            out.push('\\');
            i += 1;
            continue;
        }
        // Plain bytes: keep UTF-8 runs intact.
        let st = i;
        while raw.get(i).is_some_and(|c| *c != b'\\') {
            i += 1;
        }
        out.push_str(&String::from_utf8_lossy(raw.get(st..i).unwrap_or(&[])));
    }
    out
}

/// Someone waiting for a long parse: told how far it is, and able to stop it.
pub trait Progress {
    /// Stop at the next check.
    fn cancelled(&self) -> bool;
    /// `done` of `total` bytes read.
    fn progress(&self, stage: &'static str, done: usize, total: usize);
}

/// Parse an exchange structure.
pub fn parse(text: &str) -> Result<Exchange, Error> {
    parse_with(text, None)
}

/// [`parse`], reporting progress to `watch` and stopping when it cancels.
pub fn parse_with(text: &str, watch: Option<&dyn Progress>) -> Result<Exchange, Error> {
    parse_limited(text, watch, LIMITS)
}

/// [`parse_with`] reading a text of up to `max_bytes` (never less than [`MAX_BYTES`]); the
/// entity, record and parameter limits grow in proportion. For a caller whose user raised the
/// largest file to open (#13329).
pub fn parse_with_max_bytes(
    text: &str,
    watch: Option<&dyn Progress>,
    max_bytes: usize,
) -> Result<Exchange, Error> {
    parse_limited(text, watch, Limits::for_bytes(max_bytes))
}

/// [`parse_with`] with the counts it refuses beyond given.
fn parse_limited(
    text: &str,
    watch: Option<&dyn Progress>,
    limits: Limits,
) -> Result<Exchange, Error> {
    if text.len() > limits.bytes {
        return Err(Error::TooLarge {
            bytes: text.len(),
            limit: limits.bytes,
        });
    }
    // Some writers start the file with a UTF-8 byte order mark.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lx = Lexer {
        s: text.as_bytes(),
        i: 0,
        params: 0,
        max_params: limits.params,
        records: 0,
        max_records: limits.records,
        counted: Cell::new((0, 0)),
    };
    let magic = lx.keyword()?;
    if magic != "ISO-10303-21" {
        return Err(Error::NotStep);
    }
    lx.expect(b';')?;
    let mut ex = Exchange::default();
    loop {
        lx.ws()?;
        if lx.peek().is_none() {
            return Err(Error::MissingEnd);
        }
        let kw = lx.keyword()?;
        match kw.as_str() {
            "HEADER" => {
                lx.expect(b';')?;
                loop {
                    // The header only describes the file, and other CAD systems read past a
                    // record that does not parse (#11650): skip to the next line that starts
                    // with a keyword. If there is none, the file is not readable.
                    let start = lx.i;
                    // A header without its `ENDSEC;` must not read the DATA section as
                    // header records to skip.
                    if lx.keyword().as_deref() == Ok("DATA") {
                        return lx.fail("expected `ENDSEC;` before `DATA`");
                    }
                    lx.i = start;
                    let record = (|| {
                        let k = lx.keyword()?;
                        if k == "ENDSEC" {
                            lx.expect(b';')?;
                            return Ok(None);
                        }
                        lx.expect(b'(')?;
                        let params = lx.list_rest(1)?;
                        lx.expect(b';')?;
                        Ok(Some((k, params)))
                    })();
                    let (k, params) = match record {
                        Ok(Some(record)) => record,
                        Ok(None) => break,
                        Err(e) => {
                            let Some(next) = lx.next_header_line(start) else {
                                return Err(e);
                            };
                            lx.i = next;
                            continue;
                        }
                    };
                    match k.as_str() {
                        "FILE_SCHEMA" => {
                            if let Some(Param::List(v)) = params.first() {
                                ex.schemas = v
                                    .iter()
                                    .filter_map(|p| p.as_str().map(str::to_string))
                                    .collect();
                            }
                        }
                        "FILE_DESCRIPTION" => {
                            if let Some(Param::List(v)) = params.first() {
                                ex.description = v
                                    .iter()
                                    .filter_map(|p| p.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                ex.header.descriptions = v
                                    .iter()
                                    .filter_map(|p| p.as_str().map(str::to_string))
                                    .collect();
                            }
                            if let Some(level) = params.get(1).and_then(Param::as_str) {
                                level.clone_into(&mut ex.header.implementation_level);
                            }
                        }
                        "FILE_NAME" => {
                            let string = |i: usize| {
                                params
                                    .get(i)
                                    .and_then(Param::as_str)
                                    .unwrap_or("")
                                    .to_string()
                            };
                            let strings = |i: usize| -> Vec<String> {
                                params
                                    .get(i)
                                    .and_then(Param::as_list)
                                    .unwrap_or(&[])
                                    .iter()
                                    .filter_map(|p| p.as_str().map(str::to_string))
                                    .collect()
                            };
                            ex.header.file_name = string(0);
                            ex.header.time_stamp = string(1);
                            ex.header.authors = strings(2);
                            ex.header.organizations = strings(3);
                            ex.header.preprocessor_version = string(4);
                            ex.header.originating_system = string(5);
                            ex.header.authorization = string(6);
                            ex.preprocessor = params
                                .get(4)
                                .and_then(Param::as_str)
                                .unwrap_or("")
                                .to_string();
                            ex.originating_system = params
                                .get(5)
                                .and_then(Param::as_str)
                                .unwrap_or("")
                                .to_string();
                        }
                        _ => {}
                    }
                }
            }
            "DATA" => {
                // Optional `DATA('name', ('schema'));`
                if lx.eat(b'(')? {
                    lx.list_rest(1)?;
                }
                lx.expect(b';')?;
                loop {
                    lx.ws()?;
                    if lx.peek() != Some(b'#') {
                        let k = lx.keyword()?;
                        if k == "ENDSEC" {
                            lx.expect(b';')?;
                            break;
                        }
                        return lx.fail(&format!("unexpected `{k}` in DATA"));
                    }
                    lx.i += 1;
                    let st = lx.i;
                    while lx.peek().is_some_and(|c| c.is_ascii_digit()) {
                        lx.i += 1;
                    }
                    let id: u64 = match std::str::from_utf8(lx.s.get(st..lx.i).unwrap_or(&[]))
                        .ok()
                        .and_then(|t| t.parse().ok())
                    {
                        Some(id) => id,
                        None => return lx.fail("bad entity id"),
                    };
                    lx.expect(b'=')?;
                    let records = if lx.eat(b'(')? {
                        let mut rs = Vec::new();
                        while !lx.eat(b')')? {
                            if rs.len() > MAX_COMPLEX_PARTS {
                                return lx.fail("complex entity with too many parts");
                            }
                            rs.push(lx.record()?);
                        }
                        rs
                    } else {
                        vec![lx.record()?]
                    };
                    lx.expect(b';')?;
                    if ex.entities.len() % 4096 == 0
                        && let Some(w) = watch
                    {
                        if w.cancelled() {
                            return Err(Error::Cancelled);
                        }
                        w.progress("reading", lx.i, lx.s.len());
                    }
                    // A duplicate's records were read too, so it counts as an instance.
                    if ex.entities.len() + ex.duplicates.len() >= limits.entities {
                        return Err(Error::TooManyEntities {
                            limit: limits.entities,
                        });
                    }
                    // A duplicate id is invalid; some writers emit them for geometry nobody
                    // reads. Keep the first definition and report it.
                    match ex.entities.entry(id) {
                        Entry::Occupied(_) => ex.duplicates.push(id),
                        Entry::Vacant(v) => {
                            v.insert(Entity { records });
                        }
                    }
                }
            }
            "END-ISO-10303-21" => {
                lx.expect(b';')?;
                return Ok(ex);
            }
            // ANCHOR / REFERENCE / SIGNATURE sections (Part 21 edition 3): skip to the first
            // `ENDSEC;` in the raw text (the limit in the module documentation).
            "ANCHOR" | "REFERENCE" | "SIGNATURE" => {
                let rest = lx.s.get(lx.i..).unwrap_or(&[]);
                match rest.windows(7).position(|w| w == b"ENDSEC;") {
                    Some(p) => lx.i += p + 7,
                    None => return lx.fail("unterminated section"),
                }
            }
            _ => return lx.fail(&format!("unexpected `{kw}`")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_and_complex() {
        let t = "ISO-10303-21;\nHEADER;FILE_DESCRIPTION((''),'2;1');\nFILE_NAME('a','',(''),(''),'','sys','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;\nDATA;\n#1=CARTESIAN_POINT('p',(0.,1.5E1,-2));\n/* c */ #2=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n#3=X('it''s','\\X2\\00E9\\X0\\',$,#1,LENGTH_MEASURE(1.E-3),\"0F\");\nENDSEC;\nEND-ISO-10303-21;\n";
        let ex = parse(t).unwrap();
        assert_eq!(ex.schemas, vec!["AUTOMOTIVE_DESIGN"]);
        assert_eq!(ex.originating_system, "sys");
        let p = ex.get(1).unwrap();
        assert_eq!(p.name(), "CARTESIAN_POINT");
        let xyz: Vec<f64> = p.params()[1]
            .as_list()
            .unwrap()
            .iter()
            .filter_map(Param::as_f64)
            .collect();
        assert_eq!(xyz, vec![0.0, 15.0, -2.0]);
        let u = ex.get(2).unwrap();
        assert!(u.is_complex() && u.has("SI_UNIT"));
        assert_eq!(u.record("SI_UNIT").unwrap()[0].as_enum(), Some("MILLI"));
        let x = ex.get(3).unwrap().params();
        assert_eq!(x[0].as_str(), Some("it's"));
        assert_eq!(x[1].as_str(), Some("\u{e9}"));
        assert_eq!(x[2], Param::Null);
        assert_eq!(x[3].as_ref_id(), Some(1));
        assert_eq!(x[4].as_f64(), Some(1e-3));
    }

    /// A DATA section holding `data`, with no header.
    fn data(data: &str) -> String {
        format!("ISO-10303-21;DATA;{data}ENDSEC;END-ISO-10303-21;")
    }

    /// #11650: the header is metadata, so a malformed record in it must not stop the import.
    #[test]
    fn a_malformed_header_record_is_skipped() {
        let t = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''), '2;1');1');

FILE_NAME('','',(''),(''),'','sys','');

FILE_SCHEMA (('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('p',(0.,1.,2.));
ENDSEC;
END-ISO-10303-21;
";
        let ex = parse(t).unwrap();
        // The records after the broken one are still read.
        assert_eq!(ex.originating_system, "sys");
        assert_eq!(ex.schemas, vec!["AUTOMOTIVE_DESIGN"]);
        assert_eq!(ex.get(1).unwrap().name(), "CARTESIAN_POINT");
        // A header with nothing after the junk to resume at is still an error.
        assert!(
            parse(
                "ISO-10303-21;
HEADER;
FILE_NAME('a');junk').;ENDSEC;DATA;ENDSEC;END-ISO-10303-21;"
            )
            .is_err()
        );
    }

    /// #11650: skipping bad header records stops at DATA when the header's `ENDSEC;` is
    /// missing, so the entities are not skipped as header records and the file is an error.
    #[test]
    fn a_header_without_endsec_does_not_swallow_the_data_section() {
        for data in ["DATA;", "DATA('d', ('AUTOMOTIVE_DESIGN'));"] {
            let t = format!(
                "ISO-10303-21;\nHEADER;\nBAD;\n{data}\n#1=A();\nENDSEC;\nEND-ISO-10303-21;\n"
            );
            let e = parse(&t).unwrap_err().to_string();
            assert!(e.contains("line 4"), "{e}");
        }
    }

    /// #11650: many bad header records in a row are skipped in time that grows with the file,
    /// and the error after them still names the right line.
    #[test]
    fn many_bad_header_records_are_skipped_in_one_pass() {
        let n = 100_000;
        let bad = "BAD;\n".repeat(n);
        let ok = format!(
            "ISO-10303-21;\nHEADER;\n{bad}ENDSEC;\nDATA;\n#1=A();\nENDSEC;\nEND-ISO-10303-21;\n"
        );
        assert_eq!(parse(&ok).unwrap().get(1).unwrap().name(), "A");
        let broken =
            format!("ISO-10303-21;\nHEADER;\n{bad}DATA;\n#1=A();\nENDSEC;\nEND-ISO-10303-21;\n");
        let e = parse(&broken).unwrap_err().to_string();
        assert!(e.contains(&format!("line {}", n + 3)), "{e}");
    }

    #[test]
    fn short_entity_names_read_as_their_long_names() {
        let ex = parse(&data(
            "#1=CRTPNT('',(0.,1.,2.));#2=PLYLP('',(#1));#3=SOME_SHRT('');#4=VRTPNT('',#1);#5=FCBND('',#2,.T.);",
        ))
        .unwrap();
        let name = |id: u64| ex.get(id).map(|e| e.name().to_string());
        assert_eq!(name(1).as_deref(), Some("CARTESIAN_POINT"));
        assert_eq!(name(2).as_deref(), Some("POLY_LOOP"));
        assert_eq!(name(4).as_deref(), Some("VERTEX_POINT"));
        assert_eq!(name(5).as_deref(), Some("FACE_BOUND"));
        // A name outside the table is kept as written.
        assert_eq!(name(3).as_deref(), Some("SOME_SHRT"));
    }

    #[test]
    fn hostile_text_is_an_error() {
        for t in [
            String::new(),
            "garbage".into(),
            "ISO-10303-21;".into(),
            "ISO-10303-21;DATA;#1=A(".into(),
            "ISO-10303-21;DATA;#1=A('x".into(),
            "ISO-10303-21;DATA;#1=A(/*".into(),
            data("#=A();"),
            data("#99999999999999999999999=A();"),
            // An exponent written with a fraction is no number: `bad number`.
            data("#1=A(1e999999.);"),
            // A number needs a digit before its exponent.
            data("#1=A(.);"),
            data("#1=A(+.);"),
            data("#1=A(-.E3);"),
            // An integer beyond i64 is refused, not read as a real.
            data("#1=A(99999999999999999999);"),
        ] {
            assert!(parse(&t).is_err(), "{t:?} parsed");
        }
        // A real beyond f64 parses as infinity, which `as_f64` refuses where it is used.
        let big = parse(&data("#1=A(1.E999999);")).unwrap();
        let p = &big.get(1).unwrap().params()[0];
        assert_eq!(*p, Param::Real(f64::INFINITY));
        assert_eq!(p.as_f64(), None);
        // A byte order mark before the header is skipped.
        assert!(parse(&format!("\u{feff}{}", data(""))).is_ok());
        let deep = format!(
            "ISO-10303-21;DATA;#1=A({}{});ENDSEC;END-ISO-10303-21;",
            "(".repeat(10_000),
            ")".repeat(10_000)
        );
        assert!(parse(&deep).is_err());
        let dup = parse("ISO-10303-21;DATA;#1=A(1);#1=B();ENDSEC;END-ISO-10303-21;").unwrap();
        assert_eq!(dup.duplicates, vec![1]);
        assert_eq!(dup.get(1).unwrap().name(), "A");
        assert_eq!(decode_string(b"\\X\\E9"), "\u{e9}");
        assert_eq!(decode_string(b"a\\X2\\00E900E8\\X0\\b"), "a\u{e9}\u{e8}b");
        assert_eq!(decode_string(b"\\"), "\\");
        // One run per character, as SolidCraft's `step_out` writes them (SolidCraft #2344).
        assert_eq!(
            decode_string(b"\\X2\\5706\\X0\\\\X2\\89D2\\X0\\20"),
            "\u{5706}\u{89d2}20"
        );
        assert_eq!(
            decode_string(b"\\X4\\0001F600\\X0\\\\X2\\00E9\\X0\\\\S\\a"),
            "\u{1f600}\u{e9}\u{e1}"
        );
        // `\S\` takes a basic-alphabet character; before a UTF-8 one it is kept as written.
        assert_eq!(decode_string("\\S\\\u{e9}".as_bytes()), "\\S\\\u{e9}");
    }

    /// The decoded text, with the windows its terminator searches compare counted: the test
    /// stops at the first one beyond the text's length, which a decoder searching the same
    /// tail again reaches.
    fn decode_counted(raw: &[u8]) -> String {
        let mut steps = 0;
        decode_string_with(raw, |rest| {
            rest.windows(4).position(|w| {
                steps += 1;
                assert!(
                    steps <= raw.len(),
                    "more search steps than {} bytes",
                    raw.len()
                );
                w == b"\\X0\\"
            })
        })
    }

    #[test]
    fn run_terminators_are_searched_once() {
        // The space keeps a tag's closing backslash from pairing with the next tag's opening one
        // as an escaped `\\`, which would leave only the first tag to search from.
        for tag in [&b"\\X2\\ "[..], b"\\X4\\ "] {
            let raw = tag.repeat(100_000);
            // No `\X0\` anywhere: every tag is kept as written.
            assert_eq!(decode_counted(&raw).as_bytes(), raw.as_slice());
        }
        let raw = b"\\X2\\00E9\\X0\\".repeat(100_000);
        assert_eq!(decode_counted(&raw), "\u{e9}".repeat(100_000));
    }

    #[test]
    fn parameters_are_counted_against_the_limit() {
        // `$`, the list, its `$`, the typed value and its `1.`: five parameters.
        let t = data("#1=A($,($),T(1.));");
        let at = |params| parse_limited(&t, None, Limits { params, ..LIMITS });
        assert!(at(5).is_ok());
        assert_eq!(at(4).unwrap_err(), Error::TooManyParams { limit: 4 });
        // The count runs over the whole file, not one entity.
        let two = data("#1=A($,$);#2=A($,$);");
        assert_eq!(
            parse_limited(
                &two,
                None,
                Limits {
                    params: 3,
                    ..LIMITS
                }
            )
            .unwrap_err(),
            Error::TooManyParams { limit: 3 }
        );
    }

    #[test]
    fn records_are_counted_against_the_limit() {
        // Empty parts of complex instances hold no parameter but each is a record.
        let t = data("#1=(A()B()C());");
        let at = |records| parse_limited(&t, None, Limits { records, ..LIMITS });
        assert!(at(3).is_ok());
        assert_eq!(at(2).unwrap_err(), Error::TooManyRecords { limit: 2 });
        // The count runs over the whole file, simple instances included.
        let two = data("#1=(A()B());#2=C();");
        assert_eq!(
            parse_limited(
                &two,
                None,
                Limits {
                    records: 2,
                    ..LIMITS
                }
            )
            .unwrap_err(),
            Error::TooManyRecords { limit: 2 }
        );
    }

    #[test]
    fn a_raised_size_limit_reads_a_longer_text_and_raises_the_counts_with_it() {
        let t = data("#1=A($);");
        let small = Limits {
            bytes: t.len() - 1,
            ..LIMITS
        };
        assert_eq!(
            parse_limited(&t, None, small).unwrap_err(),
            Error::TooLarge {
                bytes: t.len(),
                limit: t.len() - 1
            }
        );
        assert!(
            parse_limited(
                &t,
                None,
                Limits {
                    bytes: t.len(),
                    ..LIMITS
                }
            )
            .is_ok()
        );
        // The message names the limit in force, not the default (#13329).
        let e = Error::TooLarge {
            bytes: 909 << 20,
            limit: 2048 << 20,
        };
        assert_eq!(e.to_string(), "STEP file too large (909 MB, limit 2048 MB)");
        // A raised size raises every count by the same factor; a lower one changes nothing.
        assert_eq!(Limits::for_bytes(0), LIMITS);
        assert_eq!(Limits::for_bytes(MAX_BYTES), LIMITS);
        assert_eq!(
            Limits::for_bytes(4 * MAX_BYTES),
            Limits {
                bytes: 4 * MAX_BYTES,
                entities: 4 * MAX_ENTITIES,
                records: 4 * MAX_RECORDS,
                params: 4 * MAX_PARAMS,
            }
        );
    }

    #[test]
    fn duplicate_instances_count_against_the_entity_limit() {
        let limits = Limits {
            entities: 2,
            ..LIMITS
        };
        assert!(parse_limited(&data("#1=A();#2=A();"), None, limits).is_ok());
        assert_eq!(
            parse_limited(&data("#1=A();#1=A();#1=A();"), None, limits).unwrap_err(),
            Error::TooManyEntities { limit: 2 }
        );
    }
}
