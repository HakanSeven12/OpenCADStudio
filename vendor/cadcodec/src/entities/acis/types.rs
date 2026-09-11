//! ACIS/SAT data types for solid modeler geometry.
//!
//! These types represent the parsed structure of ACIS SAT format data,
//! including the header, entity records, and the B-rep topology/geometry.

use std::fmt;

use super::parser::SatParser;
use super::writer::SatWriter;

// ============================================================================
// SAT Version
// ============================================================================

/// ACIS SAT version number.
///
/// Common versions:
/// - `(4, 0, 0)` → SAT version 400 (ACIS 4.0)
/// - `(7, 0, 0)` → SAT version 700 (ACIS 7.0)
/// - `(21, 0, 0)` → SAT version 21800 (ACIS 21.0)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SatVersion {
    /// Major version (e.g. 7 for ACIS 7.0).
    pub major: u32,
    /// Minor version (e.g. 0).
    pub minor: u32,
    /// Patch / sub-minor.
    pub patch: u32,
}

impl SatVersion {
    /// Creates a new SAT version.
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// ACIS 4.0 — legacy format.
    pub const V4_0: Self = Self {
        major: 4,
        minor: 0,
        patch: 0,
    };
    /// ACIS 7.0 — introduced explicit indices and asmheader.
    pub const V7_0: Self = Self {
        major: 7,
        minor: 0,
        patch: 0,
    };
    /// ACIS 21.0 — modern format.
    pub const V21_0: Self = Self {
        major: 21,
        minor: 0,
        patch: 0,
    };

    /// Returns the SAT version number used in the header line (e.g. 700).
    pub fn sat_version_number(&self) -> u32 {
        self.major * 100 + self.minor * 10 + self.patch
    }

    /// Creates a version from the SAT version number (e.g. 700 → 7.0.0).
    pub fn from_sat_number(num: u32) -> Self {
        Self {
            major: num / 100,
            minor: (num % 100) / 10,
            patch: num % 10,
        }
    }

    /// Returns true if this version uses explicit record indices (7.0+).
    pub fn has_explicit_indices(&self) -> bool {
        self.major >= 7
    }

    /// Returns true if this version uses `@`-prefixed counted strings (7.0+).
    pub fn has_counted_strings(&self) -> bool {
        self.major >= 7
    }

    /// Returns true if this version requires an `asmheader` entity (7.0+).
    pub fn has_asm_header(&self) -> bool {
        self.major >= 7
    }
}

impl Default for SatVersion {
    fn default() -> Self {
        Self::V7_0
    }
}

impl fmt::Display for SatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ============================================================================
// SAT Header
// ============================================================================

/// Header of a SAT file.
///
/// The first 3 lines of a SAT file contain version info, product info,
/// and modeler tolerances.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SatHeader {
    /// ACIS version.
    pub version: SatVersion,
    /// Number of entity records.
    pub num_records: usize,
    /// Number of bodies.
    pub num_bodies: usize,
    /// Whether history data is present.
    pub has_history: bool,
    /// Product identifier string.
    pub product_id: String,
    /// Product version string.
    pub product_version: String,
    /// File creation date string.
    pub date: String,
    /// Spatial resolution (minimum edge length, typically 1e-06).
    pub spatial_resolution: f64,
    /// Normal tolerance (angular tolerance in radians, typically ~1e-07).
    pub normal_tolerance: f64,
    /// Fit tolerance for approximation (ACIS 7.0+, typically 1e-10).
    pub resfit_tolerance: Option<f64>,
}

impl SatHeader {
    /// Creates a default header for ACIS 7.0.
    pub fn new() -> Self {
        Self {
            version: SatVersion::V7_0,
            num_records: 0,
            num_bodies: 0,
            has_history: false,
            product_id: "acadrust".to_string(),
            product_version: "ACIS 7.0".to_string(),
            date: "Thu Jan 01 00:00:00 2023".to_string(),
            spatial_resolution: 10.0,
            normal_tolerance: 9.9999999999999995e-07,
            resfit_tolerance: Some(1e-10),
        }
    }

    /// Creates a header with the specified version.
    pub fn with_version(version: SatVersion) -> Self {
        let mut header = Self::new();
        header.version = version;
        header.product_version = format!("ACIS {}.{}", version.major, version.minor);
        header
    }
}

impl Default for SatHeader {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Entity Pointer
// ============================================================================

/// A pointer/reference to another SAT entity record.
///
/// In SAT text, pointers appear as `$<index>` (e.g. `$3`) or `$-1` for null.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SatPointer(pub i32);

impl SatPointer {
    /// Null pointer (`$-1`).
    pub const NULL: Self = Self(-1);

    /// Creates a pointer to the given record index.
    pub fn new(index: i32) -> Self {
        Self(index)
    }

    /// Returns true if this is a null pointer.
    pub fn is_null(&self) -> bool {
        self.0 < 0
    }

    /// Returns the index, or `None` if null.
    pub fn index(&self) -> Option<usize> {
        if self.0 >= 0 {
            Some(self.0 as usize)
        } else {
            None
        }
    }
}

impl Default for SatPointer {
    fn default() -> Self {
        Self::NULL
    }
}

impl fmt::Display for SatPointer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.0)
    }
}

// ============================================================================
// SAT Token
// ============================================================================

/// A single token in a SAT entity record.
///
/// SAT records consist of a sequence of tokens separated by spaces.
/// Tokens can be entity-type identifiers, pointers, numbers, strings,
/// or enum values.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub enum SatToken {
    /// An identifier/keyword (entity type, sense, etc.).
    Ident(String),
    /// A pointer reference (`$<index>`).
    Pointer(SatPointer),
    /// An integer value.
    Integer(i64),
    /// A floating-point value.
    Float(f64),
    /// A counted string (`@<len> <text>`).
    String(String),
    /// A position/point value `(x y z)`.
    Position(f64, f64, f64),
    /// The boolean true literal.
    True,
    /// The boolean false literal.
    False,
    /// The record terminator `#`.
    Terminator,
    /// An enum-like keyword (forward, reversed, single, double, etc.).
    Enum(String),
    /// A losslessly preserved SAB token. `data` contains every byte after
    /// `tag`, including any length prefix, exactly as it appeared in the
    /// binary stream.
    Sab {
        /// SAB type tag.
        tag: u8,
        /// Raw tag payload.
        data: Vec<u8>,
    },
}

/// Serde representation for [`SatToken`].
///
/// Coordinate SAB tags retain their raw bytes in memory so a SAB writer can
/// reproduce them exactly. JSON consumers, however, have historically seen
/// them as the semantic `Position` token shared with SAT input.
#[cfg(feature = "serde")]
#[derive(serde::Serialize)]
enum SatTokenJson<'a> {
    Ident(&'a str),
    Pointer(SatPointer),
    Integer(i64),
    Float(f64),
    String(&'a str),
    Position(f64, f64, f64),
    True,
    False,
    Terminator,
    Enum(&'a str),
    Sab { tag: u8, data: &'a [u8] },
}

#[cfg(feature = "serde")]
impl serde::Serialize for SatToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let token = match self {
            Self::Ident(value) => SatTokenJson::Ident(value),
            Self::Pointer(value) => SatTokenJson::Pointer(*value),
            Self::Integer(value) => SatTokenJson::Integer(*value),
            Self::Float(value) => SatTokenJson::Float(*value),
            Self::String(value) => SatTokenJson::String(value),
            Self::Position(x, y, z) => SatTokenJson::Position(*x, *y, *z),
            Self::True => SatTokenJson::True,
            Self::False => SatTokenJson::False,
            Self::Terminator => SatTokenJson::Terminator,
            Self::Enum(value) => SatTokenJson::Enum(value),
            Self::Sab { tag, data } => match self.coordinate_components() {
                Some(([x, y, z], 3)) => SatTokenJson::Position(x, y, z),
                _ => SatTokenJson::Sab { tag: *tag, data },
            },
        };
        serde::Serialize::serialize(&token, serializer)
    }
}

impl SatToken {
    /// Returns the token as a string if it is an identifier.
    pub fn as_ident(&self) -> Option<&str> {
        match self {
            SatToken::Ident(s) | SatToken::Enum(s) => Some(s),
            SatToken::Sab { tag: 0x0F, .. } => Some("{"),
            SatToken::Sab { tag: 0x10, .. } => Some("}"),
            SatToken::Sab { tag, data } => {
                sab_string_bytes(*tag, data).and_then(|bytes| std::str::from_utf8(bytes).ok())
            }
            _ => None,
        }
    }

    /// Returns the token as a pointer if it is one.
    pub fn as_pointer(&self) -> Option<SatPointer> {
        match self {
            SatToken::Pointer(p) => Some(*p),
            _ => None,
        }
    }

    /// Returns the token as an integer if it is one.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            SatToken::Integer(v) => Some(*v),
            SatToken::Float(v) => exact_integer(*v),
            SatToken::Sab { tag: 0x02, data } if data.len() == 1 => {
                Some(i8::from_le_bytes([data[0]]) as i64)
            }
            SatToken::Sab { tag: 0x03, data } if data.len() == 2 => {
                Some(i16::from_le_bytes([data[0], data[1]]) as i64)
            }
            SatToken::Sab {
                tag: 0x04 | 0x15,
                data,
            } if data.len() == 4 => {
                Some(i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as i64)
            }
            SatToken::Sab { tag: 0x17, data } if data.len() == 8 => Some(i64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ])),
            SatToken::Sab { tag: 0x05, data } if data.len() == 4 => {
                exact_integer(f32::from_le_bytes([data[0], data[1], data[2], data[3]]) as f64)
            }
            SatToken::Sab { tag: 0x06, data } if data.len() == 8 => {
                exact_integer(f64::from_le_bytes([
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ]))
            }
            _ => None,
        }
    }

    /// Returns the token as a float if it is one.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            SatToken::Float(v) => Some(*v),
            SatToken::Integer(v) => Some(*v as f64),
            SatToken::Sab { tag: 0x02, data } if data.len() == 1 => {
                Some(i8::from_le_bytes([data[0]]) as f64)
            }
            SatToken::Sab { tag: 0x03, data } if data.len() == 2 => {
                Some(i16::from_le_bytes([data[0], data[1]]) as f64)
            }
            SatToken::Sab {
                tag: 0x04 | 0x15,
                data,
            } if data.len() == 4 => {
                Some(i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as f64)
            }
            SatToken::Sab { tag: 0x05, data } if data.len() == 4 => {
                Some(f32::from_le_bytes([data[0], data[1], data[2], data[3]]) as f64)
            }
            SatToken::Sab { tag: 0x06, data } if data.len() == 8 => Some(f64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ])),
            SatToken::Sab { tag: 0x17, data } if data.len() == 8 => Some(i64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]) as f64),
            _ => None,
        }
    }

    /// Returns the token as a string value.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            SatToken::String(s) => Some(s),
            SatToken::Sab { tag, data } => {
                sab_string_bytes(*tag, data).and_then(|bytes| std::str::from_utf8(bytes).ok())
            }
            _ => None,
        }
    }

    /// Returns the coordinate components carried by a SAT/SAB vector token.
    ///
    /// The returned length is two for SAB U-V vectors and three for positions
    /// and directions.
    pub fn coordinate_components(&self) -> Option<([f64; 3], usize)> {
        match self {
            SatToken::Position(x, y, z) => Some(([*x, *y, *z], 3)),
            SatToken::Sab {
                tag: 0x13 | 0x14,
                data,
            } if data.len() == 24 => Some((
                [
                    f64::from_le_bytes(data[0..8].try_into().ok()?),
                    f64::from_le_bytes(data[8..16].try_into().ok()?),
                    f64::from_le_bytes(data[16..24].try_into().ok()?),
                ],
                3,
            )),
            SatToken::Sab { tag: 0x16, data } if data.len() == 16 => Some((
                [
                    f64::from_le_bytes(data[0..8].try_into().ok()?),
                    f64::from_le_bytes(data[8..16].try_into().ok()?),
                    0.0,
                ],
                2,
            )),
            _ => None,
        }
    }
}

fn exact_integer(value: f64) -> Option<i64> {
    (value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64)
        .then_some(value as i64)
}

fn sab_string_bytes(tag: u8, data: &[u8]) -> Option<&[u8]> {
    let prefix = match tag {
        0x07 | 0x0D | 0x0E => 1,
        0x08 => 2,
        0x09 | 0x12 => 4,
        _ => return None,
    };
    (data.len() >= prefix).then_some(&data[prefix..])
}

impl fmt::Display for SatToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SatToken::Ident(s) => write!(f, "{}", s),
            SatToken::Pointer(p) => write!(f, "{}", p),
            SatToken::Integer(v) => write!(f, "{}", v),
            SatToken::Float(v) => {
                if v.fract() == 0.0 && !v.is_infinite() && !v.is_nan() {
                    write!(f, "{:.1}", v)
                } else {
                    write!(f, "{}", v)
                }
            }
            SatToken::String(s) => write!(f, "@{} {}", s.len(), s),
            SatToken::Position(x, y, z) => write!(f, "{} {} {}", x, y, z),
            SatToken::True => write!(f, "TRUE"),
            SatToken::False => write!(f, "FALSE"),
            SatToken::Terminator => write!(f, "#"),
            SatToken::Enum(s) => write!(f, "{}", s),
            SatToken::Sab { tag, data } => {
                if let Some((values, len)) = self.coordinate_components() {
                    for (index, value) in values[..len].iter().enumerate() {
                        if index != 0 {
                            write!(f, " ")?;
                        }
                        write!(f, "{}", value)?;
                    }
                    Ok(())
                } else if let Some(value) = self.as_integer() {
                    write!(f, "{}", value)
                } else if let Some(value) = self.as_float() {
                    write!(f, "{}", value)
                } else if let Some(value) =
                    sab_string_bytes(*tag, data).and_then(|bytes| std::str::from_utf8(bytes).ok())
                {
                    if matches!(*tag, 0x0D | 0x0E) {
                        write!(f, "{}", value)
                    } else {
                        write!(f, "@{} {}", value.len(), value)
                    }
                } else {
                    match *tag {
                        0x0F => write!(f, "{{"),
                        0x10 => write!(f, "}}"),
                        _ => write!(f, "<sab:{:02X}:{}>", tag, data.len()),
                    }
                }
            }
        }
    }
}

// ============================================================================
// SAT Entity Record
// ============================================================================

/// A single entity record in a SAT file.
///
/// Each record represents a topological or geometric entity in the B-rep model.
/// Records are terminated by `#` in the text format.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SatRecord {
    /// Record index (explicit in ACIS 7.0+, implicit/sequential in earlier).
    pub index: i32,
    /// Entity type name (e.g. "body", "face", "plane-surface").
    pub entity_type: String,
    /// Entity sub-type (for entities like "xxx-surface", "xxx-curve").
    pub sub_type: Option<String>,
    /// Attribute pointer (first pointer after entity type).
    pub attribute: SatPointer,
    /// Subtype/ID field (integer after attribute, always -1 for standard entities).
    pub subtype_id: i32,
    /// Remaining tokens in the record (after entity type, attribute, and subtype_id).
    pub tokens: Vec<SatToken>,
    /// Raw text of the record (preserved for roundtrip fidelity).
    pub raw_text: Option<String>,
}

impl SatRecord {
    /// Creates a new empty record.
    pub fn new(index: i32, entity_type: &str) -> Self {
        Self {
            index,
            entity_type: entity_type.to_string(),
            sub_type: None,
            attribute: SatPointer::NULL,
            subtype_id: -1,
            tokens: Vec::new(),
            raw_text: None,
        }
    }

    /// Returns whether this record is the requested ACIS class or derives
    /// from it through a hyphen-separated SAB class name.
    pub fn is_a(&self, entity_type: &str) -> bool {
        self.entity_type == entity_type || base_entity_type(&self.entity_type) == entity_type
    }

    /// Returns all pointer references in this record.
    pub fn pointers(&self) -> Vec<SatPointer> {
        let mut ptrs = vec![self.attribute];
        for token in &self.tokens {
            if let SatToken::Pointer(p) = token {
                ptrs.push(*p);
            }
        }
        ptrs
    }

    /// Returns the token at the given position (0-based, after the attribute).
    pub fn token(&self, index: usize) -> Option<&SatToken> {
        self.tokens.get(index)
    }

    /// Returns the integer value of the token at the given position.
    pub fn token_integer(&self, index: usize) -> Option<i64> {
        self.tokens.get(index).and_then(|t| t.as_integer())
    }

    /// Returns the float value of the token at the given position.
    pub fn token_float(&self, index: usize) -> Option<f64> {
        // Coordinate-aware flat-float indexing. SAT text stores a point/vector
        // as three separate float tokens, so the geometry accessors read
        // px,py,pz / nx,ny,nz at consecutive indices. SAB/ASM (AutoCAD 2013+)
        // packs each coordinate triple into ONE Position/Direction token, so
        // expand those into three consecutive float slots here. Records with
        // no Position tokens (all SAT, most SAB scalars) index 1:1 as before.
        let mut slot = 0usize;
        for t in &self.tokens {
            if let Some((components, len)) = t.coordinate_components() {
                for &c in &components[..len] {
                    if slot == index {
                        return Some(c);
                    }
                    slot += 1;
                }
            } else {
                if slot == index {
                    return t.as_float();
                }
                slot += 1;
            }
        }
        None
    }

    /// Returns the pointer at the given token position.
    pub fn token_pointer(&self, index: usize) -> Option<SatPointer> {
        self.tokens.get(index).and_then(|t| t.as_pointer())
    }

    /// Pointer by ordinal: the `index`-th pointer token, skipping interleaved
    /// scalar tokens. Most accessors use absolute positions (`token_pointer`)
    /// because ACIS keeps parameters in fixed slots, but a few records gained
    /// an extra scalar field in ASM (ShapeManager, AutoCAD 2013+) — e.g. a
    /// vertex's tolerance int between its edge and point — where ordinal
    /// indexing stays correct for both ACIS and ASM.
    pub fn nth_pointer(&self, index: usize) -> Option<SatPointer> {
        self.tokens.iter().filter_map(|t| t.as_pointer()).nth(index)
    }

    /// Returns the string value at the given token position.
    pub fn token_string(&self, index: usize) -> Option<&str> {
        self.tokens
            .get(index)
            .and_then(|token| token.as_ident().or_else(|| token.as_string()))
    }

    /// Read an edge/coedge sense from token `index`, accepting both the
    /// keyword form (`forward` / `reversed`, ACIS 4.0+) and the numeric form
    /// pre-4.0 SAT uses (`0` = forward, `1` = reversed) — the latter tokenizes
    /// as an Integer, so `token_string` misses it and every coedge defaults to
    /// forward, collapsing reversed-coedge loops to a duplicate vertex.
    pub fn token_sense(&self, index: usize) -> Sense {
        match self.tokens.get(index) {
            Some(token) if token.as_integer().is_some() => {
                if token.as_integer().unwrap_or(0) != 0 {
                    Sense::Reversed
                } else {
                    Sense::Forward
                }
            }
            _ => self
                .token_string(index)
                .map(Sense::from_str)
                .unwrap_or_default(),
        }
    }
}

impl fmt::Display for SatRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.index, self.entity_type, self.attribute)?;
        for token in &self.tokens {
            write!(f, " {}", token)?;
        }
        write!(f, " #")
    }
}

// ============================================================================
// SAT Entity Types (Typed Accessors)
// ============================================================================

/// The sense of a surface normal or curve direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Sense {
    /// Forward sense.
    Forward,
    /// Reversed sense.
    Reversed,
}

impl Sense {
    /// Parse from SAT token.
    pub fn from_str(s: &str) -> Self {
        match s {
            // Keyword form (ACIS 4.0+) and the numeric form pre-4.0 SAT uses
            // (0 = forward, 1 = reversed).
            "reversed" | "REVERSED" | "reversed_v" | "REVERSED_V" | "reverse_v" | "REVERSE_V"
            | "1" => Self::Reversed,
            _ => Self::Forward,
        }
    }

    /// Returns the SAT string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Reversed => "reversed",
        }
    }
}

impl Default for Sense {
    fn default() -> Self {
        Self::Forward
    }
}

/// Surface sidedness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Sidedness {
    /// Single-sided surface.
    Single,
    /// Double-sided surface.
    Double,
}

impl Sidedness {
    /// Parse from SAT token.
    pub fn from_str(s: &str) -> Self {
        match s {
            "double" | "DOUBLE" => Self::Double,
            _ => Self::Single,
        }
    }

    /// Returns the SAT string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Double => "double",
        }
    }
}

impl Default for Sidedness {
    fn default() -> Self {
        Self::Single
    }
}

// ============================================================================
// Typed Entity Accessors
// ============================================================================

/// Accessor for a `body` entity record.
///
/// Body record layout: `body $<attrib> <id> $<next_body> $<lump> $<wire> $<transform>`
#[derive(Debug, Clone)]
pub struct SatBody<'a> {
    record: &'a SatRecord,
}

impl<'a> SatBody<'a> {
    /// Wraps a record as a body entity. Returns None if not a body.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.is_a("body") {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Pointer to the next body.
    pub fn next_body(&self) -> SatPointer {
        self.record.token_pointer(0).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the first lump.
    pub fn lump(&self) -> SatPointer {
        self.record.token_pointer(1).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the wire body (if any).
    pub fn wire_body(&self) -> SatPointer {
        self.record.token_pointer(2).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the transform.
    pub fn transform(&self) -> SatPointer {
        self.record.token_pointer(3).unwrap_or(SatPointer::NULL)
    }
}

/// Accessor for a `lump` entity record.
///
/// Lump record layout: `lump $<attrib> $<next_lump> $<shell> $<body>`
#[derive(Debug, Clone)]
pub struct SatLump<'a> {
    record: &'a SatRecord,
}

impl<'a> SatLump<'a> {
    /// Wraps a record as a lump entity. Returns None if not a lump.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.is_a("lump") {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Pointer to the next lump.
    pub fn next_lump(&self) -> SatPointer {
        self.record.token_pointer(1).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the shell.
    pub fn shell(&self) -> SatPointer {
        self.record.token_pointer(2).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the owner body.
    pub fn body(&self) -> SatPointer {
        self.record.token_pointer(3).unwrap_or(SatPointer::NULL)
    }
}

/// Accessor for a `shell` entity record.
///
/// Shell record layout: `shell $<attrib> $<next_shell> $<subshell> $<face> $<wire> $<lump>`
#[derive(Debug, Clone)]
pub struct SatShell<'a> {
    record: &'a SatRecord,
}

impl<'a> SatShell<'a> {
    /// Wraps a record as a shell entity. Returns None if not a shell.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.is_a("shell") {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Pointer to the next shell.
    pub fn next_shell(&self) -> SatPointer {
        self.record.token_pointer(1).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the subshell.
    pub fn subshell(&self) -> SatPointer {
        self.record.token_pointer(2).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the first face.
    pub fn face(&self) -> SatPointer {
        self.record.token_pointer(3).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the wire.
    pub fn wire(&self) -> SatPointer {
        self.record.token_pointer(4).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the owner lump.
    pub fn lump(&self) -> SatPointer {
        self.record.token_pointer(5).unwrap_or(SatPointer::NULL)
    }
}

/// Accessor for a `subshell` topology record.
///
/// ACIS/ASM releases extend this record differently, so links after the
/// common pattern pointer are exposed by ordinal instead of assigning an
/// unsafe version-specific meaning to them.
#[derive(Debug, Clone)]
pub struct SatSubshell<'a> {
    record: &'a SatRecord,
}

impl<'a> SatSubshell<'a> {
    /// Wraps a record as a subshell entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.is_a("subshell") {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Pattern-feature pointer introduced in newer ACIS save versions.
    pub fn pattern(&self) -> SatPointer {
        self.record.token_pointer(0).unwrap_or(SatPointer::NULL)
    }

    /// Returns a topology link after the common pattern pointer.
    pub fn link(&self, ordinal: usize) -> SatPointer {
        self.record
            .nth_pointer(ordinal + 1)
            .unwrap_or(SatPointer::NULL)
    }

    /// Underlying lossless record.
    pub fn record(&self) -> &'a SatRecord {
        self.record
    }
}

/// Accessor for a `wire` topology record.
///
/// Wire payloads changed across ACIS/ASM versions. The stable pattern pointer
/// is named; subsequent topology links remain ordinal and lossless.
#[derive(Debug, Clone)]
pub struct SatWire<'a> {
    record: &'a SatRecord,
}

impl<'a> SatWire<'a> {
    /// Wraps a record as a wire entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.is_a("wire") {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Pattern-feature pointer introduced in newer ACIS save versions.
    pub fn pattern(&self) -> SatPointer {
        self.record.token_pointer(0).unwrap_or(SatPointer::NULL)
    }

    /// Returns a topology link after the common pattern pointer.
    pub fn link(&self, ordinal: usize) -> SatPointer {
        self.record
            .nth_pointer(ordinal + 1)
            .unwrap_or(SatPointer::NULL)
    }

    /// Underlying lossless record.
    pub fn record(&self) -> &'a SatRecord {
        self.record
    }
}

/// Accessor for any class derived from ACIS `attrib`.
///
/// Attribute common data is stable: next attribute, previous attribute and
/// owner pointers, followed by class-specific payload.
#[derive(Debug, Clone)]
pub struct SatAttribute<'a> {
    record: &'a SatRecord,
}

impl<'a> SatAttribute<'a> {
    /// Wraps a derived attribute record.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if classify_entity_type(&record.entity_type) == SatEntityCategory::Attribute {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Next attribute in the owner's attribute chain.
    pub fn next(&self) -> SatPointer {
        self.record.token_pointer(0).unwrap_or(SatPointer::NULL)
    }

    /// Previous attribute in the owner's attribute chain.
    pub fn previous(&self) -> SatPointer {
        self.record.token_pointer(1).unwrap_or(SatPointer::NULL)
    }

    /// Entity which owns this attribute.
    pub fn owner(&self) -> SatPointer {
        self.record.token_pointer(2).unwrap_or(SatPointer::NULL)
    }

    /// Class-specific payload after the three common attribute links.
    pub fn payload(&self) -> &'a [SatToken] {
        self.record.tokens.get(3..).unwrap_or(&[])
    }

    /// Underlying lossless record.
    pub fn record(&self) -> &'a SatRecord {
        self.record
    }
}

/// Accessor for a `face` entity record.
///
/// Face record layout: `face $<attrib> <id> $<unknown> $<next_face> $<loop> $<shell> $<subshell> $<surface> <sense> <sidedness>`
#[derive(Debug, Clone)]
pub struct SatFace<'a> {
    record: &'a SatRecord,
}

impl<'a> SatFace<'a> {
    /// Wraps a record as a face entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.is_a("face") {
            Some(Self { record })
        } else {
            None
        }
    }

    /// First attribute attached to this face.
    pub fn attribute(&self) -> SatPointer {
        self.record.attribute
    }

    /// Underlying lossless face record.
    pub fn record(&self) -> &'a SatRecord {
        self.record
    }

    /// Pointer to the next face.
    pub fn next_face(&self) -> SatPointer {
        self.record.token_pointer(1).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the first loop.
    pub fn first_loop(&self) -> SatPointer {
        self.record.token_pointer(2).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the owner shell.
    pub fn shell(&self) -> SatPointer {
        self.record.token_pointer(3).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the subshell.
    pub fn subshell(&self) -> SatPointer {
        self.record.token_pointer(4).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the surface geometry.
    pub fn surface(&self) -> SatPointer {
        self.record.token_pointer(5).unwrap_or(SatPointer::NULL)
    }

    /// Surface sense.
    pub fn sense(&self) -> Sense {
        self.record
            .token_string(6)
            .map(Sense::from_str)
            .unwrap_or_default()
    }

    /// Surface sidedness.
    pub fn sidedness(&self) -> Sidedness {
        self.record
            .token_string(7)
            .map(Sidedness::from_str)
            .unwrap_or_default()
    }
}

/// Accessor for a `loop` entity record.
///
/// Loop record layout: `loop $<attrib> <id> $<pattern> $<next_loop> $<coedge> $<face>`
///
/// The leading `$<pattern>` pointer is the ACIS pattern-feature reference
/// (present from the PATTERN save version on, NULL in practice). There is no
/// outer-vs-hole / loop-type field: ACIS does not record which loop is the
/// outer boundary and which are holes — the kernel classifies them at runtime
/// from geometry (coedge winding vs the face's outward normal). Consumers must
/// derive the distinction themselves.
#[derive(Debug, Clone)]
pub struct SatLoop<'a> {
    record: &'a SatRecord,
}

impl<'a> SatLoop<'a> {
    /// Wraps a record as a loop entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.is_a("loop") {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Pointer to the next loop in the face's loop list. The first token is the
    /// ACIS pattern-feature pointer (NULL in practice); the next-loop link is
    /// the second. Loop order is not significant — the outer boundary is not
    /// guaranteed to come first.
    pub fn next_loop(&self) -> SatPointer {
        self.record.token_pointer(1).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the first coedge.
    pub fn first_coedge(&self) -> SatPointer {
        self.record.token_pointer(2).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the owner face.
    pub fn face(&self) -> SatPointer {
        self.record.token_pointer(3).unwrap_or(SatPointer::NULL)
    }
}

/// Accessor for a `coedge` entity record.
///
/// Coedge record layout: `coedge $<attrib> $<next> $<prev> $<partner> $<edge> <sense> $<loop>`
#[derive(Debug, Clone)]
pub struct SatCoedge<'a> {
    record: &'a SatRecord,
}

impl<'a> SatCoedge<'a> {
    /// Wraps a record as a coedge entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.is_a("coedge") {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Pointer to the next coedge in the loop.
    pub fn next(&self) -> SatPointer {
        self.record.token_pointer(1).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the previous coedge in the loop.
    pub fn prev(&self) -> SatPointer {
        self.record.token_pointer(2).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the partner coedge (on adjacent face).
    pub fn partner(&self) -> SatPointer {
        self.record.token_pointer(3).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the edge.
    pub fn edge(&self) -> SatPointer {
        self.record.token_pointer(4).unwrap_or(SatPointer::NULL)
    }

    /// Sense of this coedge relative to the edge.
    pub fn sense(&self) -> Sense {
        self.record.token_sense(5)
    }

    /// Pointer to the owner loop.
    pub fn owner_loop(&self) -> SatPointer {
        self.record.token_pointer(6).unwrap_or(SatPointer::NULL)
    }

    /// Optional parametric curve on the owner face's surface.
    pub fn pcurve(&self) -> SatPointer {
        self.record.nth_pointer(6).unwrap_or(SatPointer::NULL)
    }
}

/// Accessor for an `edge` entity record.
///
/// Edge record layout: `edge $<attrib> $<start_vertex> $<end_vertex> $<coedge> $<curve> <sense>`
#[derive(Debug, Clone)]
pub struct SatEdge<'a> {
    record: &'a SatRecord,
}

impl<'a> SatEdge<'a> {
    /// Wraps a record as an edge entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.is_a("edge") {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Pointer to the start vertex.
    pub fn start_vertex(&self) -> SatPointer {
        self.record.token_pointer(1).unwrap_or(SatPointer::NULL)
    }

    /// Start parameter along the curve.
    pub fn start_param(&self) -> f64 {
        self.record.token_float(2).unwrap_or(0.0)
    }

    /// Pointer to the end vertex.
    pub fn end_vertex(&self) -> SatPointer {
        self.record.token_pointer(3).unwrap_or(SatPointer::NULL)
    }

    /// End parameter along the curve.
    pub fn end_param(&self) -> f64 {
        self.record.token_float(4).unwrap_or(0.0)
    }

    /// Pointer to the first coedge using this edge.
    pub fn coedge(&self) -> SatPointer {
        self.record.token_pointer(5).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the curve geometry.
    pub fn curve(&self) -> SatPointer {
        self.record.token_pointer(6).unwrap_or(SatPointer::NULL)
    }

    /// Edge sense.
    pub fn sense(&self) -> Sense {
        self.record.token_sense(7)
    }
}

/// Accessor for a `vertex` entity record.
///
/// Vertex record layout: `vertex $<attrib> $<edge> $<point>`
#[derive(Debug, Clone)]
pub struct SatVertex<'a> {
    record: &'a SatRecord,
}

impl<'a> SatVertex<'a> {
    /// Wraps a record as a vertex entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.is_a("vertex") {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Pointer to the edge.
    pub fn edge(&self) -> SatPointer {
        self.record.nth_pointer(1).unwrap_or(SatPointer::NULL)
    }

    /// Pointer to the point geometry.
    ///
    /// Uses pointer-ordinal indexing so the ASM (AutoCAD 2013+) vertex layout
    /// `$attr $edge <tolerance:int> $point` resolves the same as the classic
    /// ACIS `$attr $edge $point`.
    pub fn point(&self) -> SatPointer {
        self.record.nth_pointer(2).unwrap_or(SatPointer::NULL)
    }
}

// ============================================================================
// Geometry Entity Accessors
// ============================================================================

/// Accessor for a `point` entity record.
///
/// Point record layout: `point $<attrib> <x> <y> <z>`
#[derive(Debug, Clone)]
pub struct SatPoint<'a> {
    record: &'a SatRecord,
}

impl<'a> SatPoint<'a> {
    /// Wraps a record as a point entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.entity_type == "point" {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Returns the coordinates as (x, y, z).
    pub fn position(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(1).unwrap_or(0.0);
        let y = self.record.token_float(2).unwrap_or(0.0);
        let z = self.record.token_float(3).unwrap_or(0.0);
        (x, y, z)
    }
}

/// Accessor for a `straight-curve` entity record.
///
/// Straight-curve layout: `straight-curve $<attrib> <px> <py> <pz> <dx> <dy> <dz> ...`
#[derive(Debug, Clone)]
pub struct SatStraightCurve<'a> {
    record: &'a SatRecord,
}

impl<'a> SatStraightCurve<'a> {
    /// Wraps a record as a straight-curve entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.entity_type == "straight-curve" {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Root point (position on the line).
    pub fn root_point(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(1).unwrap_or(0.0);
        let y = self.record.token_float(2).unwrap_or(0.0);
        let z = self.record.token_float(3).unwrap_or(0.0);
        (x, y, z)
    }

    /// Direction vector.
    pub fn direction(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(4).unwrap_or(0.0);
        let y = self.record.token_float(5).unwrap_or(0.0);
        let z = self.record.token_float(6).unwrap_or(1.0);
        (x, y, z)
    }
}

/// Accessor for an `ellipse-curve` entity record.
///
/// Ellipse-curve layout: `ellipse-curve $<attrib> <cx> <cy> <cz> <nx> <ny> <nz> <major_x> <major_y> <major_z> <ratio> ...`
#[derive(Debug, Clone)]
pub struct SatEllipseCurve<'a> {
    record: &'a SatRecord,
}

impl<'a> SatEllipseCurve<'a> {
    /// Wraps a record as an ellipse-curve entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.entity_type == "ellipse-curve" {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Center point.
    pub fn center(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(1).unwrap_or(0.0);
        let y = self.record.token_float(2).unwrap_or(0.0);
        let z = self.record.token_float(3).unwrap_or(0.0);
        (x, y, z)
    }

    /// Normal vector.
    pub fn normal(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(4).unwrap_or(0.0);
        let y = self.record.token_float(5).unwrap_or(0.0);
        let z = self.record.token_float(6).unwrap_or(1.0);
        (x, y, z)
    }

    /// Major axis direction.
    pub fn major_axis(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(7).unwrap_or(1.0);
        let y = self.record.token_float(8).unwrap_or(0.0);
        let z = self.record.token_float(9).unwrap_or(0.0);
        (x, y, z)
    }

    /// Ratio of minor to major axis.
    pub fn ratio(&self) -> f64 {
        self.record.token_float(10).unwrap_or(1.0)
    }
}

/// Accessor for `intcurve-curve`, `bs3-curve` and `exactcur-curve` records.
/// Their geometry is an embedded `nubs` or rational `nurbs` block:
/// `... <degree> <form> <n_knots> [<knot> <mult>]* [<x> <y> <z> [w]]* ...`.
/// Straight and elliptic edges have their own dedicated accessors; this covers
/// the general spline edges (fillet/blend boundaries), which otherwise decode
/// to nothing and leave a face un-trimmable.
#[derive(Debug, Clone)]
pub struct SatIntCurve<'a> {
    record: &'a SatRecord,
}

/// Direct bs3-curve records use the interpolated-curve B-spline accessor.
pub type SatBs3Curve<'a> = SatIntCurve<'a>;

impl<'a> SatIntCurve<'a> {
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        let entity_type = record.entity_type.as_str();
        if matches!(
            entity_type,
            "intcurve-curve" | "bs3-curve" | "exactcur-curve"
        ) || entity_type.ends_with("-intcurve-curve")
            || entity_type.ends_with("-bs3-curve")
            || entity_type.ends_with("-exactcur-curve")
        {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Parse the embedded spline block into homogeneous control points.
    pub fn bspline(&self) -> Option<(usize, Vec<f64>, Vec<[f64; 4]>)> {
        decode_bspline_curve(&self.record.tokens)
    }

    /// Parse an inline or shared spline definition.
    pub fn bspline_in(&self, document: &SatDocument) -> Option<(usize, Vec<f64>, Vec<[f64; 4]>)> {
        self.bspline().or_else(|| {
            let reference = subtype_reference(&self.record.tokens)?;
            decode_bspline_curve(document.subtype_tokens(reference)?)
        })
    }

    /// Whether the spline definition is periodic or closed.
    pub fn is_closed_in(&self, document: &SatDocument) -> bool {
        let tokens = if let Some(reference) = subtype_reference(&self.record.tokens) {
            match document.subtype_tokens(reference) {
                Some(tokens) => tokens,
                None => return false,
            }
        } else {
            &self.record.tokens
        };
        let Some(start) = tokens
            .iter()
            .position(|token| matches!(token.as_ident(), Some("nubs" | "nurbs")))
        else {
            return false;
        };
        tokens.get(start + 2).is_some_and(|token| {
            matches!(token.as_ident(), Some("closed" | "periodic"))
                || token.as_integer().is_some_and(|value| value != 0)
        })
    }

    /// Sample `segs + 1` points evenly along the curve's full valid parameter
    /// range. Empty when the record has no decodable nubs geometry.
    pub fn sample(&self, segs: usize) -> Vec<(f64, f64, f64)> {
        let Some((degree, knots, _)) = self.bspline() else {
            return Vec::new();
        };
        self.sample_range(knots[degree], knots[knots.len() - degree - 1], segs)
    }

    /// Sample `segs + 1` points evenly along `[t0, t1]` — an edge's own
    /// parameter span, so only the used arc of the curve is emitted. The range
    /// is clamped to the curve's valid parameter interval. Empty when the record
    /// has no decodable nubs geometry or the span is degenerate.
    pub fn sample_range(&self, t0: f64, t1: f64, segs: usize) -> Vec<(f64, f64, f64)> {
        let Some((degree, knots, ctrl)) = self.bspline() else {
            return Vec::new();
        };
        let segs = segs.max(1);
        let (lo, hi) = (knots[degree], knots[knots.len() - degree - 1]);
        let (a, b) = (t0.min(t1).max(lo), t0.max(t1).min(hi));
        if !(b > a) {
            return Vec::new();
        }
        (0..=segs)
            .map(|s| {
                let t = a + (b - a) * (s as f64 / segs as f64);
                let p = de_boor_homogeneous(degree, &knots, &ctrl, t);
                let w = if p[3].abs() > 1e-12 { p[3] } else { 1.0 };
                (p[0] / w, p[1] / w, p[2] / w)
            })
            .collect()
    }
}

fn decode_bspline_curve(t: &[SatToken]) -> Option<(usize, Vec<f64>, Vec<[f64; 4]>)> {
    let ni = t
        .iter()
        .position(|x| matches!(x.as_ident(), Some("nubs" | "nurbs")))?;
    let rational = t.get(ni)?.as_ident() == Some("nurbs");
    let degree = t.get(ni + 1)?.as_integer()? as usize;
    // t[ni + 2] is the curve form/closure flag.
    let n_knots = t.get(ni + 3)?.as_integer()? as usize;
    let mut knots: Vec<f64> = Vec::new();
    let mut i = ni + 4;
    for _ in 0..n_knots {
        let k = t.get(i)?.as_float()?;
        let mult = t.get(i + 1)?.as_integer()?.max(0) as usize;
        for _ in 0..mult {
            knots.push(k);
        }
        i += 2;
    }
    if knots.len() < 2 {
        return None;
    }
    let stride = if rational { 4 } else { 3 };
    let available = t[i..]
        .iter()
        .take_while(|token| token.as_float().is_some())
        .count()
        / stride;
    let base_ctrl = knots.len().checked_sub(degree + 1)?;
    let n_ctrl = if available >= base_ctrl + 2 {
        let first = *knots.first()?;
        let last = *knots.last()?;
        knots.insert(0, first);
        knots.push(last);
        base_ctrl + 2
    } else {
        base_ctrl
    };
    if n_ctrl <= degree {
        return None;
    }
    let mut ctrl: Vec<[f64; 4]> = Vec::with_capacity(n_ctrl);
    for _ in 0..n_ctrl {
        let x = t.get(i)?.as_float()?;
        let y = t.get(i + 1)?.as_float()?;
        let z = t.get(i + 2)?.as_float()?;
        if rational {
            let w = t.get(i + 3)?.as_float()?;
            ctrl.push([x * w, y * w, z * w, w]);
            i += 4;
        } else {
            ctrl.push([x, y, z, 1.0]);
            i += 3;
        }
    }
    Some((degree, knots, ctrl))
}

/// Accessor for a `pcurve` entity: a 2-D B-spline in surface UV space.
#[derive(Debug, Clone)]
pub struct SatPCurve<'a> {
    record: &'a SatRecord,
}

impl<'a> SatPCurve<'a> {
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.entity_type == "pcurve" {
            Some(Self { record })
        } else {
            None
        }
    }

    fn bspline_at(tokens: &[SatToken], start: usize) -> Option<(usize, Vec<f64>, Vec<[f64; 4]>)> {
        let rational = tokens.get(start)?.as_ident() == Some("nurbs");
        let degree = tokens.get(start + 1)?.as_integer()?.max(0) as usize;
        let knot_count = tokens.get(start + 3)?.as_integer()?.max(0) as usize;
        let mut position = start + 4;
        let mut knots = Vec::new();
        for _ in 0..knot_count {
            let value = tokens.get(position)?.as_float()?;
            let multiplicity = tokens.get(position + 1)?.as_integer()?.max(0) as usize;
            for _ in 0..multiplicity {
                knots.push(value);
            }
            position += 2;
        }
        if knots.len() < 2 {
            return None;
        }
        let stride = if rational { 3 } else { 2 };
        let available = tokens[position..]
            .iter()
            .take_while(|token| token.as_float().is_some())
            .count()
            / stride;
        let base_count = knots.len().checked_sub(degree + 1)?;
        let control_count = if available >= base_count + 2 {
            let first = *knots.first()?;
            let last = *knots.last()?;
            knots.insert(0, first);
            knots.push(last);
            base_count + 2
        } else {
            base_count
        };
        if control_count <= degree {
            return None;
        }
        let mut control = Vec::with_capacity(control_count);
        for _ in 0..control_count {
            let u = tokens.get(position)?.as_float()?;
            let v = tokens.get(position + 1)?.as_float()?;
            if rational {
                let weight = tokens.get(position + 2)?.as_float()?;
                control.push([u * weight, v * weight, 0.0, weight]);
                position += 3;
            } else {
                control.push([u, v, 0.0, 1.0]);
                position += 2;
            }
        }
        Some((degree, knots, control))
    }

    fn sample_bspline(
        spline: (usize, Vec<f64>, Vec<[f64; 4]>),
        segments: usize,
    ) -> Vec<(f64, f64)> {
        let (degree, knots, control) = spline;
        let start = knots[degree];
        let end = knots[knots.len() - degree - 1];
        if end <= start {
            return Vec::new();
        }
        let segments = segments.max(1);
        (0..=segments)
            .map(|index| {
                let parameter = start + (end - start) * (index as f64 / segments as f64);
                let point = de_boor_homogeneous(degree, &knots, &control, parameter);
                let weight = if point[3].abs() > 1e-12 {
                    point[3]
                } else {
                    1.0
                };
                (point[0] / weight, point[1] / weight)
            })
            .collect()
    }

    fn apply_offsets(&self, points: &mut [(f64, f64)]) {
        let Some((u_offset, v_offset)) = self.offsets() else {
            return;
        };
        for point in points {
            point.0 += u_offset;
            point.1 += v_offset;
        }
    }

    fn offsets(&self) -> Option<(f64, f64)> {
        let offsets: Vec<f64> = self
            .record
            .tokens
            .iter()
            .rev()
            .filter_map(SatToken::as_float)
            .take(2)
            .collect();
        (offsets.len() == 2).then_some((offsets[1], offsets[0]))
    }

    fn referenced_bspline(
        &self,
        document: &SatDocument,
    ) -> Option<((usize, Vec<f64>, Vec<[f64; 4]>), bool)> {
        let selector = self
            .record
            .tokens
            .iter()
            .find_map(SatToken::as_integer)
            .unwrap_or(1);
        let target = self
            .record
            .nth_pointer(1)
            .and_then(|pointer| document.resolve(pointer))?;
        let mut tokens = target.tokens.as_slice();
        if !tokens
            .iter()
            .any(|token| token.as_ident() == Some("par_int_cur"))
        {
            if let Some(reference) = subtype_reference(tokens) {
                tokens = document.subtype_tokens(reference)?;
            }
        }
        let parametric = tokens
            .iter()
            .any(|token| token.as_ident() == Some("par_int_cur"));
        let blocks: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| {
                matches!(token.as_ident(), Some("nubs" | "nurbs")).then_some(index)
            })
            .collect();
        let support = selector.unsigned_abs().max(1) as usize;
        let block = support.checked_sub(usize::from(parametric))?;
        let spline = Self::bspline_at(tokens, *blocks.get(block)?)?;
        let reversed = (selector < 0) ^ (target.token_sense(1) == Sense::Reversed);
        Some((spline, reversed))
    }

    /// Decode this UV curve into homogeneous `[uw, vw, w]` controls.
    pub fn bspline_in(&self, document: &SatDocument) -> Option<(usize, Vec<f64>, Vec<[f64; 3]>)> {
        let direct = self
            .record
            .tokens
            .iter()
            .position(|token| matches!(token.as_ident(), Some("nubs" | "nurbs")))
            .and_then(|start| Self::bspline_at(&self.record.tokens, start));
        let (spline, reversed) = match direct {
            Some(spline) => (spline, false),
            None => self.referenced_bspline(document)?,
        };
        let (degree, mut knots, controls) = spline;
        let (u_offset, v_offset) = self.offsets().unwrap_or((0.0, 0.0));
        let mut controls: Vec<[f64; 3]> = controls
            .into_iter()
            .map(|point| {
                [
                    point[0] + u_offset * point[3],
                    point[1] + v_offset * point[3],
                    point[3],
                ]
            })
            .collect();
        if reversed {
            controls.reverse();
            let sum = knots.get(degree)? + knots.get(controls.len())?;
            knots = knots.into_iter().rev().map(|knot| sum - knot).collect();
        }
        Some((degree, knots, controls))
    }

    /// Sample the complete UV curve.
    pub fn sample(&self, segments: usize) -> Vec<(f64, f64)> {
        let Some(start) = self
            .record
            .tokens
            .iter()
            .position(|token| matches!(token.as_ident(), Some("nubs" | "nurbs")))
        else {
            return Vec::new();
        };
        let Some(spline) = Self::bspline_at(&self.record.tokens, start) else {
            return Vec::new();
        };
        let mut points = Self::sample_bspline(spline, segments);
        self.apply_offsets(&mut points);
        points
    }

    /// Sample a UV curve which may reference an `intcurve-curve` subtype.
    ///
    /// ACIS shares large interpolated-curve subtypes through `{ ref n }`.
    /// A pcurve then stores a signed 1-based support-surface index, a pointer
    /// to the owning intcurve, and periodic U/V offsets. Direct inline pcurves
    /// remain supported by [`Self::sample`].
    pub fn sample_in(&self, document: &SatDocument, segments: usize) -> Vec<(f64, f64)> {
        let direct = self.sample(segments);
        if !direct.is_empty() {
            return direct;
        }

        let Some((spline, reversed)) = self.referenced_bspline(document) else {
            return Vec::new();
        };
        let mut points = Self::sample_bspline(spline, segments);
        if reversed {
            points.reverse();
        }
        self.apply_offsets(&mut points);
        points
    }
}

fn subtype_reference(tokens: &[SatToken]) -> Option<usize> {
    let start = tokens
        .iter()
        .position(|token| token.as_ident() == Some("{"))?;
    if tokens.get(start + 1).and_then(SatToken::as_ident) != Some("ref")
        || tokens.get(start + 3).and_then(SatToken::as_ident) != Some("}")
    {
        return None;
    }
    tokens
        .get(start + 2)?
        .as_integer()
        .and_then(|index| usize::try_from(index).ok())
}

/// Decoded `nubs`/`nurbs` control net shared by spline, meshsurf and bs3
/// surface records. Control points are homogeneous `[xw, yw, zw, w]`.
#[derive(Debug, Clone, PartialEq)]
pub struct SatBSplineSurface {
    pub rational: bool,
    pub degree_u: usize,
    pub degree_v: usize,
    pub u_closure: Option<String>,
    pub v_closure: Option<String>,
    pub u_singularity: Option<String>,
    pub v_singularity: Option<String>,
    pub u_knots: Vec<f64>,
    pub v_knots: Vec<f64>,
    pub control_count_u: usize,
    pub control_count_v: usize,
    pub control_points: Vec<[f64; 4]>,
    pub fit_tolerance: Option<f64>,
}

/// Surface family represented by [`SatSplineSurface`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SatSplineSurfaceKind {
    Spline,
    Mesh,
    Bs3,
}

/// Typed accessor for `spline-surface`, `meshsurf-surface` and `bs3-surface`.
///
/// It resolves shared `{ ref n }` subtype definitions and decodes both SAT
/// scalar control nets and SAB grouped position tokens.
#[derive(Debug, Clone)]
pub struct SatSplineSurface<'a> {
    record: &'a SatRecord,
    kind: SatSplineSurfaceKind,
}

/// Meshsurf records use the same B-spline payload accessor.
pub type SatMeshSurface<'a> = SatSplineSurface<'a>;
/// Direct bs3-surface records use the same B-spline payload accessor.
pub type SatBs3Surface<'a> = SatSplineSurface<'a>;

impl<'a> SatSplineSurface<'a> {
    /// Wraps any supported B-spline surface family.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        let entity_type = record.entity_type.as_str();
        let kind =
            if entity_type == "meshsurf-surface" || entity_type.ends_with("-meshsurf-surface") {
                SatSplineSurfaceKind::Mesh
            } else if entity_type == "bs3-surface" || entity_type.ends_with("-bs3-surface") {
                SatSplineSurfaceKind::Bs3
            } else if entity_type == "spline-surface" || entity_type.ends_with("-spline-surface") {
                SatSplineSurfaceKind::Spline
            } else {
                return None;
            };
        Some(Self { record, kind })
    }

    /// Concrete surface family.
    pub fn kind(&self) -> SatSplineSurfaceKind {
        self.kind
    }

    /// Surface parameterization sense.
    pub fn sense(&self) -> Sense {
        self.record.token_sense(1)
    }

    /// Underlying lossless record.
    pub fn record(&self) -> &'a SatRecord {
        self.record
    }

    /// Inline subtype tokens, or the shared definition addressed by `{ ref n }`.
    pub fn definition_tokens<'b>(&'b self, document: &'b SatDocument) -> Option<&'b [SatToken]> {
        if let Some(reference) = subtype_reference(&self.record.tokens) {
            document.subtype_tokens(reference)
        } else {
            Some(&self.record.tokens)
        }
    }

    /// Decode the final `nubs`/`nurbs` block into a complete control net.
    pub fn bspline(&self, document: &SatDocument) -> Option<SatBSplineSurface> {
        decode_bspline_surface(self.definition_tokens(document)?)
    }
}

fn decode_bspline_surface(tokens: &[SatToken]) -> Option<SatBSplineSurface> {
    let start = tokens
        .iter()
        .rposition(|token| matches!(token.as_ident(), Some("nubs" | "nurbs")))?;
    let rational = tokens.get(start)?.as_ident() == Some("nurbs");
    let degree_u = usize::try_from(tokens.get(start + 1)?.as_integer()?).ok()?;
    let degree_v = usize::try_from(tokens.get(start + 2)?.as_integer()?).ok()?;

    let string_at = |index: usize| {
        tokens
            .get(index)
            .and_then(SatToken::as_ident)
            .map(str::to_owned)
    };
    let mut form_start = start + 3;
    if rational
        && matches!(
            tokens.get(form_start).and_then(SatToken::as_ident),
            Some("u" | "v" | "both" | "none")
        )
    {
        form_start += 1;
    }
    let u_closure = string_at(form_start);
    let v_closure = string_at(form_start + 1);
    let u_singularity = string_at(form_start + 2);
    let v_singularity = string_at(form_start + 3);
    let u_knot_count = usize::try_from(tokens.get(form_start + 4)?.as_integer()?).ok()?;
    let v_knot_count = usize::try_from(tokens.get(form_start + 5)?.as_integer()?).ok()?;
    let mut position = form_start + 6;

    let mut u_knots = Vec::new();
    for _ in 0..u_knot_count {
        let value = tokens.get(position)?.as_float()?;
        let multiplicity = usize::try_from(tokens.get(position + 1)?.as_integer()?).ok()?;
        u_knots.extend(std::iter::repeat(value).take(multiplicity));
        position += 2;
    }
    let mut v_knots = Vec::new();
    for _ in 0..v_knot_count {
        let value = tokens.get(position)?.as_float()?;
        let multiplicity = usize::try_from(tokens.get(position + 1)?.as_integer()?).ok()?;
        v_knots.extend(std::iter::repeat(value).take(multiplicity));
        position += 2;
    }
    if u_knots.len() < 2 || v_knots.len() < 2 {
        return None;
    }

    let mut values = Vec::new();
    for token in &tokens[position..] {
        if token.as_ident() == Some("}") {
            break;
        }
        if let Some((components, len)) = token.coordinate_components() {
            values.extend_from_slice(&components[..len]);
        } else if let Some(value) = token.as_float() {
            values.push(value);
        } else if let Some(value) = token.as_integer() {
            values.push(value as f64);
        }
    }

    let stride = if rational { 4 } else { 3 };
    let base_u = u_knots.len().checked_sub(degree_u + 1)?;
    let base_v = v_knots.len().checked_sub(degree_v + 1)?;
    let mut best: Option<(usize, usize, bool, bool, usize)> = None;
    for clamp_u in [false, true] {
        for clamp_v in [false, true] {
            let count_u = base_u + usize::from(clamp_u) * 2;
            let count_v = base_v + usize::from(clamp_v) * 2;
            if count_u <= degree_u || count_v <= degree_v {
                continue;
            }
            let needed = count_u.checked_mul(count_v)?.checked_mul(stride)?;
            if needed > values.len() {
                continue;
            }
            let remaining = values.len() - needed;
            if best
                .as_ref()
                .map(|candidate| remaining < candidate.4)
                .unwrap_or(true)
            {
                best = Some((count_u, count_v, clamp_u, clamp_v, remaining));
            }
        }
    }
    let (control_count_u, control_count_v, clamp_u, clamp_v, _) = best?;
    if clamp_u {
        let first = *u_knots.first()?;
        let last = *u_knots.last()?;
        u_knots.insert(0, first);
        u_knots.push(last);
    }
    if clamp_v {
        let first = *v_knots.first()?;
        let last = *v_knots.last()?;
        v_knots.insert(0, first);
        v_knots.push(last);
    }

    let control_scalar_count = control_count_u
        .checked_mul(control_count_v)?
        .checked_mul(stride)?;
    let mut control_points = Vec::with_capacity(control_count_u * control_count_v);
    for point in values[..control_scalar_count].chunks_exact(stride) {
        if rational {
            let weight = point[3];
            control_points.push([
                point[0] * weight,
                point[1] * weight,
                point[2] * weight,
                weight,
            ]);
        } else {
            control_points.push([point[0], point[1], point[2], 1.0]);
        }
    }

    Some(SatBSplineSurface {
        rational,
        degree_u,
        degree_v,
        u_closure,
        v_closure,
        u_singularity,
        v_singularity,
        u_knots,
        v_knots,
        control_count_u,
        control_count_v,
        control_points,
        fit_tolerance: values.get(control_scalar_count).copied(),
    })
}

/// Evaluate a homogeneous B-spline curve at parameter `t` via De Boor.
fn de_boor_homogeneous(degree: usize, knots: &[f64], ctrl: &[[f64; 4]], t: f64) -> [f64; 4] {
    // Knot span k: knots[k] <= t < knots[k+1], clamped into the valid interval.
    let n = ctrl.len();
    let mut k = degree;
    while k + 1 < knots.len() && knots[k + 1] <= t && k + 1 <= n {
        k += 1;
    }
    k = k.min(n - 1).max(degree);
    // Working control points for the recursion.
    let mut d: Vec<[f64; 4]> = (0..=degree)
        .map(|j| ctrl[(k - degree + j).min(n - 1)])
        .collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = k - degree + j;
            let denom = knots[i + degree - r + 1] - knots[i];
            let a = if denom.abs() > 1e-12 {
                (t - knots[i]) / denom
            } else {
                0.0
            };
            for c in 0..4 {
                d[j][c] = (1.0 - a) * d[j - 1][c] + a * d[j][c];
            }
        }
    }
    d[degree]
}

/// Accessor for a `plane-surface` entity record.
///
/// Plane-surface layout: `plane-surface $<attrib> <px> <py> <pz> <nx> <ny> <nz> <ux> <uy> <uz> ...`
#[derive(Debug, Clone)]
pub struct SatPlaneSurface<'a> {
    record: &'a SatRecord,
}

impl<'a> SatPlaneSurface<'a> {
    /// Wraps a record as a plane-surface entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.entity_type == "plane-surface" {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Root point on the plane.
    pub fn root_point(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(1).unwrap_or(0.0);
        let y = self.record.token_float(2).unwrap_or(0.0);
        let z = self.record.token_float(3).unwrap_or(0.0);
        (x, y, z)
    }

    /// Normal vector.
    pub fn normal(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(4).unwrap_or(0.0);
        let y = self.record.token_float(5).unwrap_or(0.0);
        let z = self.record.token_float(6).unwrap_or(1.0);
        (x, y, z)
    }

    /// U direction on the surface.
    pub fn u_direction(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(7).unwrap_or(1.0);
        let y = self.record.token_float(8).unwrap_or(0.0);
        let z = self.record.token_float(9).unwrap_or(0.0);
        (x, y, z)
    }

    pub fn sense(&self) -> Sense {
        self.record.token_sense(10)
    }
}

/// Accessor for a `cone-surface` entity record.
///
/// Cone-surface layout (v700):
/// `cone-surface $<attrib> -1 $-1 <cx> <cy> <cz> <ax_x> <ax_y> <ax_z> <rx> <ry> <rz> <ratio> I I <sin_half_angle> <cos_half_angle> <radius> forward_v I I I I`
///
/// Tokens 11–12 are spline continuation markers (`I`), so sine/cosine
/// sit at positions 13 and 14. For a cylinder, sin=0 and cos=1.
#[derive(Debug, Clone)]
pub struct SatConeSurface<'a> {
    record: &'a SatRecord,
}

impl<'a> SatConeSurface<'a> {
    /// Wraps a record as a cone-surface entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.entity_type == "cone-surface" {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Center point (apex or center of base circle).
    pub fn center(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(1).unwrap_or(0.0);
        let y = self.record.token_float(2).unwrap_or(0.0);
        let z = self.record.token_float(3).unwrap_or(0.0);
        (x, y, z)
    }

    /// Axis direction.
    pub fn axis(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(4).unwrap_or(0.0);
        let y = self.record.token_float(5).unwrap_or(0.0);
        let z = self.record.token_float(6).unwrap_or(1.0);
        (x, y, z)
    }

    /// Major radius direction.
    pub fn major_axis(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(7).unwrap_or(1.0);
        let y = self.record.token_float(8).unwrap_or(0.0);
        let z = self.record.token_float(9).unwrap_or(0.0);
        (x, y, z)
    }

    /// Ratio of minor to major radius.
    pub fn ratio(&self) -> f64 {
        self.record.token_float(10).unwrap_or(1.0)
    }

    /// Sine of half angle (position 13, after two `I` continuation tokens).
    /// For a cylinder this is 0.0.
    pub fn sin_half_angle(&self) -> f64 {
        self.record.token_float(13).unwrap_or(0.0)
    }

    /// Cosine of half angle (position 14, after two `I` continuation tokens).
    /// For a cylinder this is 1.0.
    pub fn cos_half_angle(&self) -> f64 {
        self.record.token_float(14).unwrap_or(1.0)
    }

    /// Radius at the reference cross-section (position 15).
    pub fn radius(&self) -> f64 {
        self.record.token_float(15).unwrap_or(1.0)
    }

    pub fn sense(&self) -> Sense {
        self.record.token_sense(16)
    }
}

/// Accessor for a `sphere-surface` entity record.
///
/// Sphere-surface layout: `sphere-surface $<attrib> <cx> <cy> <cz> <radius> <ux> <uy> <uz> <pole_x> <pole_y> <pole_z> ...`
#[derive(Debug, Clone)]
pub struct SatSphereSurface<'a> {
    record: &'a SatRecord,
}

impl<'a> SatSphereSurface<'a> {
    /// Wraps a record as a sphere-surface entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.entity_type == "sphere-surface" {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Center point.
    pub fn center(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(1).unwrap_or(0.0);
        let y = self.record.token_float(2).unwrap_or(0.0);
        let z = self.record.token_float(3).unwrap_or(0.0);
        (x, y, z)
    }

    /// Radius.
    pub fn radius(&self) -> f64 {
        self.record.token_float(4).unwrap_or(1.0)
    }

    /// U direction.
    pub fn u_direction(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(5).unwrap_or(1.0);
        let y = self.record.token_float(6).unwrap_or(0.0);
        let z = self.record.token_float(7).unwrap_or(0.0);
        (x, y, z)
    }

    /// Pole direction.
    pub fn pole(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(8).unwrap_or(0.0);
        let y = self.record.token_float(9).unwrap_or(0.0);
        let z = self.record.token_float(10).unwrap_or(1.0);
        (x, y, z)
    }

    pub fn sense(&self) -> Sense {
        self.record.token_sense(11)
    }
}

/// Accessor for a `torus-surface` entity record.
///
/// Torus-surface layout: `torus-surface $<attrib> <cx> <cy> <cz> <nx> <ny> <nz> <major_r> <minor_r> <ux> <uy> <uz> ...`
#[derive(Debug, Clone)]
pub struct SatTorusSurface<'a> {
    record: &'a SatRecord,
}

impl<'a> SatTorusSurface<'a> {
    /// Wraps a record as a torus-surface entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.entity_type == "torus-surface" {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Center point.
    pub fn center(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(1).unwrap_or(0.0);
        let y = self.record.token_float(2).unwrap_or(0.0);
        let z = self.record.token_float(3).unwrap_or(0.0);
        (x, y, z)
    }

    /// Normal (axis of revolution).
    pub fn normal(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(4).unwrap_or(0.0);
        let y = self.record.token_float(5).unwrap_or(0.0);
        let z = self.record.token_float(6).unwrap_or(1.0);
        (x, y, z)
    }

    /// Major radius (distance from center to tube center).
    pub fn major_radius(&self) -> f64 {
        self.record.token_float(7).unwrap_or(1.0)
    }

    /// Minor radius (tube radius).
    pub fn minor_radius(&self) -> f64 {
        self.record.token_float(8).unwrap_or(0.5)
    }

    /// U direction.
    pub fn u_direction(&self) -> (f64, f64, f64) {
        let x = self.record.token_float(9).unwrap_or(1.0);
        let y = self.record.token_float(10).unwrap_or(0.0);
        let z = self.record.token_float(11).unwrap_or(0.0);
        (x, y, z)
    }

    pub fn sense(&self) -> Sense {
        self.record.token_sense(12)
    }
}

/// Accessor for a `transform` entity record.
///
/// Transform layout (3x3 + translation + scale):
/// `transform $<attrib> <r00> <r01> <r02> <r10> <r11> <r12> <r20> <r21> <r22> <tx> <ty> <tz> <scale> <is_rotation> <is_reflection> <is_shear>`
#[derive(Debug, Clone)]
pub struct SatTransform<'a> {
    record: &'a SatRecord,
}

impl<'a> SatTransform<'a> {
    /// Wraps a record as a transform entity.
    pub fn from_record(record: &'a SatRecord) -> Option<Self> {
        if record.entity_type == "transform" {
            Some(Self { record })
        } else {
            None
        }
    }

    /// Rotation matrix row 0 (X axis).
    pub fn row0(&self) -> (f64, f64, f64) {
        (
            self.record.token_float(1).unwrap_or(1.0),
            self.record.token_float(2).unwrap_or(0.0),
            self.record.token_float(3).unwrap_or(0.0),
        )
    }

    /// Rotation matrix row 1 (Y axis).
    pub fn row1(&self) -> (f64, f64, f64) {
        (
            self.record.token_float(4).unwrap_or(0.0),
            self.record.token_float(5).unwrap_or(1.0),
            self.record.token_float(6).unwrap_or(0.0),
        )
    }

    /// Rotation matrix row 2 (Z axis).
    pub fn row2(&self) -> (f64, f64, f64) {
        (
            self.record.token_float(7).unwrap_or(0.0),
            self.record.token_float(8).unwrap_or(0.0),
            self.record.token_float(9).unwrap_or(1.0),
        )
    }

    /// Translation vector.
    pub fn translation(&self) -> (f64, f64, f64) {
        (
            self.record.token_float(10).unwrap_or(0.0),
            self.record.token_float(11).unwrap_or(0.0),
            self.record.token_float(12).unwrap_or(0.0),
        )
    }

    /// Scale factor.
    pub fn scale(&self) -> f64 {
        self.record.token_float(13).unwrap_or(1.0)
    }
}

// ============================================================================
// SAT Document
// ============================================================================

/// A parsed SAT document representing complete ACIS solid model data.
///
/// This is the main entry point for working with ACIS geometry. Parse raw
/// SAT text from a Solid3D/Region/Body entity's `AcisData`, inspect or
/// modify the B-rep topology, and write back to SAT text.
///
/// # Example
///
/// ```rust
/// use acadrust::entities::acis::SatDocument;
///
/// let sat_text = "700 0 1 0\n\
///     @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
///     1e-06 9.9999999999999995e-07\n\
///     -0 asmheader $-1 -1 @12 700 7 0 0 @5 ACIS @3 7.0 @24 Thu Jan 01 00:00:00 2023 #\n\
///     -1 body $-1 $-1 $-1 $-1 #\n\
///     End-of-ACIS-data\n";
///
/// let doc = SatDocument::parse(sat_text).unwrap();
/// assert!(doc.records.len() >= 2);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SatDocument {
    /// SAT file header.
    pub header: SatHeader,
    /// Entity records in the document.
    pub records: Vec<SatRecord>,
}

impl SatDocument {
    /// Creates a new empty SAT document with ACIS 7.0 header.
    pub fn new() -> Self {
        Self {
            header: SatHeader::new(),
            records: Vec::new(),
        }
    }

    /// Creates a document with the specified header.
    pub fn with_header(header: SatHeader) -> Self {
        Self {
            header,
            records: Vec::new(),
        }
    }

    /// Parses SAT text into a structured document.
    pub fn parse(text: &str) -> Result<Self, SatParseError> {
        SatParser::parse(text)
    }

    /// Writes the document back to SAT text format.
    pub fn to_sat_string(&self) -> String {
        SatWriter::write(self)
    }

    /// Read the body placement as `(matrix_rowmajor, translation, scale)` in
    /// the SAT convention `world = scale·(p·M) + T`. Returns identity when the
    /// document has no `transform` record. The first 13 numeric tokens of the
    /// transform record carry the 3×3, translation and scale; the leading
    /// book-keeping pointer and trailing rotate/reflect/shear flags are
    /// skipped by reading float-valued tokens only.
    pub fn placement(&self) -> ([[f64; 3]; 3], [f64; 3], f64) {
        const IDENTITY: ([[f64; 3]; 3], [f64; 3], f64) = (
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.0; 3],
            1.0,
        );
        let Some(rec) = self.records.iter().find(|r| r.entity_type == "transform") else {
            return IDENTITY;
        };
        // SAT text tokenizes the 13-number payload (3×3 matrix, translation,
        // scale) as individual floats, but the SAB reader groups the matrix
        // rows and the translation into `Position` triplets — flatten both.
        let mut v: Vec<f64> = Vec::with_capacity(13);
        for tok in &rec.tokens {
            if v.len() >= 13 {
                break;
            }
            if let Some((components, len)) = tok.coordinate_components() {
                v.extend_from_slice(&components[..len]);
            } else if let Some(f) = tok.as_float() {
                v.push(f);
            } else if let Some(text) = tok.as_string() {
                // ShapeManager stores the complete numeric transform payload
                // in one ASM long-string token (0x12) in some SAB files.
                // It is still the regular SAT transform sequence, followed
                // by no_rotate/no_reflect/no_shear.
                for word in text.split_ascii_whitespace() {
                    let Ok(value) = word.parse::<f64>() else {
                        break;
                    };
                    v.push(value);
                    if v.len() >= 13 {
                        break;
                    }
                }
            }
        }
        if v.len() < 13 {
            return IDENTITY;
        }
        (
            [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]],
            [v[9], v[10], v[11]],
            v[12],
        )
    }

    /// Set the body placement, creating the `transform` record (and wiring the
    /// body's transform pointer) when absent. Encodes the 3×3, translation and
    /// scale in the layout `placement()` reads back.
    pub fn set_placement(&mut self, matrix: [[f64; 3]; 3], translation: [f64; 3], scale: f64) {
        let mut tokens = Vec::with_capacity(17);
        tokens.push(SatToken::Pointer(SatPointer::NULL)); // v700 book-keeping
        for row in &matrix {
            for &x in row {
                tokens.push(SatToken::Float(x));
            }
        }
        for &x in &translation {
            tokens.push(SatToken::Float(x));
        }
        tokens.push(SatToken::Float(scale));
        // rotate / reflect / shear — a composed matrix may be non-orthogonal,
        // so flag it as a general (shear) placement.
        tokens.push(SatToken::Integer(0));
        tokens.push(SatToken::Integer(0));
        tokens.push(SatToken::Integer(1));

        if let Some(rec) = self
            .records
            .iter_mut()
            .find(|r| r.entity_type == "transform")
        {
            rec.tokens = tokens;
            return;
        }
        // No transform record yet — append one. Records are position-indexed,
        // and the `End-of-ACIS-data` / `End-of-ASM-data` terminator must stay
        // last (the parser stops there). Lift any terminator off, push the
        // transform, then restore the terminator so it remains final;
        // otherwise the new record sits past the terminator and is dropped on
        // the next parse.
        let terminator = self
            .records
            .iter()
            .position(|r| r.entity_type.starts_with("End-of"))
            .map(|p| self.records.remove(p));
        let index = self.records.len() as i32;
        let mut rec = SatRecord::new(index, "transform");
        rec.attribute = SatPointer::NULL;
        rec.tokens = tokens;
        self.records.push(rec);
        // Wire the first body's transform pointer (4th pointer token:
        // next_body, lump, wire, transform).
        if let Some(body) = self.records.iter_mut().find(|r| r.entity_type == "body") {
            if body.tokens.len() >= 4 {
                body.tokens[3] = SatToken::Pointer(SatPointer::new(index));
            }
        }
        if let Some(term) = terminator {
            self.records.push(term);
        }
        self.header.num_records = self.records.len();
    }

    /// Returns the number of entity records.
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Returns a record by index.
    ///
    /// Records are parsed and appended in index order, so slot `index` almost
    /// always holds the record whose `.index == index` — an O(1) hit. Only a
    /// document whose records were re-ordered or hand-built falls back to the
    /// linear scan. Tessellation resolves O(records) pointers per solid, so the
    /// old unconditional scan made every solid O(records²) — the dominant cost
    /// of meshing an ACIS-heavy drawing.
    pub fn record(&self, index: usize) -> Option<&SatRecord> {
        if let Some(r) = self.records.get(index) {
            if r.index == index as i32 {
                return Some(r);
            }
        }
        self.records.iter().find(|r| r.index == index as i32)
    }

    /// Returns a mutable record by index.
    pub fn record_mut(&mut self, index: usize) -> Option<&mut SatRecord> {
        if matches!(self.records.get(index), Some(r) if r.index == index as i32) {
            return self.records.get_mut(index);
        }
        self.records.iter_mut().find(|r| r.index == index as i32)
    }

    /// Returns all records of a given entity type.
    pub fn records_of_type(&self, entity_type: &str) -> Vec<&SatRecord> {
        self.records
            .iter()
            .filter(|r| r.entity_type == entity_type)
            .collect()
    }

    /// Returns all body records.
    pub fn bodies(&self) -> Vec<SatBody<'_>> {
        self.records
            .iter()
            .filter_map(SatBody::from_record)
            .collect()
    }

    /// Returns all subshell records.
    pub fn subshells(&self) -> Vec<SatSubshell<'_>> {
        self.records
            .iter()
            .filter_map(SatSubshell::from_record)
            .collect()
    }

    /// Returns all wire topology records.
    pub fn wires(&self) -> Vec<SatWire<'_>> {
        self.records
            .iter()
            .filter_map(SatWire::from_record)
            .collect()
    }

    /// Returns all derived attribute records.
    pub fn attributes(&self) -> Vec<SatAttribute<'_>> {
        self.records
            .iter()
            .filter_map(SatAttribute::from_record)
            .collect()
    }

    /// Returns all face records.
    pub fn faces(&self) -> Vec<SatFace<'_>> {
        self.records
            .iter()
            .filter_map(SatFace::from_record)
            .collect()
    }

    /// Returns all edge records.
    pub fn edges(&self) -> Vec<SatEdge<'_>> {
        self.records
            .iter()
            .filter_map(SatEdge::from_record)
            .collect()
    }

    /// Returns all vertex records.
    pub fn vertices(&self) -> Vec<SatVertex<'_>> {
        self.records
            .iter()
            .filter_map(SatVertex::from_record)
            .collect()
    }

    /// Returns all B-spline surface-family records.
    pub fn spline_surfaces(&self) -> Vec<SatSplineSurface<'_>> {
        self.records
            .iter()
            .filter_map(SatSplineSurface::from_record)
            .collect()
    }

    /// Adds a record and returns its index.
    pub fn add_record(&mut self, mut record: SatRecord) -> i32 {
        let index = self.records.len() as i32;
        record.index = index;
        self.records.push(record);
        self.header.num_records = self.records.len();
        index
    }

    /// Returns the record that a pointer refers to.
    pub fn resolve(&self, ptr: SatPointer) -> Option<&SatRecord> {
        if ptr.is_null() {
            None
        } else {
            self.record(ptr.0 as usize)
        }
    }

    /// Resolve a shared ACIS subtype definition by its file-global index.
    ///
    /// Subtypes are numbered in encounter order across all entity records.
    /// `{ ref n }` blocks reference an earlier definition and do not consume
    /// an index of their own. The returned slice excludes the surrounding
    /// braces but includes the subtype identifier and its complete payload.
    pub fn subtype_tokens(&self, requested: usize) -> Option<&[SatToken]> {
        let mut current = 0usize;
        for record in &self.records {
            let tokens = &record.tokens;
            for start in 0..tokens.len() {
                if tokens[start].as_ident() != Some("{")
                    || tokens.get(start + 1).and_then(SatToken::as_ident) == Some("ref")
                {
                    continue;
                }
                if current == requested {
                    let mut depth = 1usize;
                    for end in start + 1..tokens.len() {
                        match tokens[end].as_ident() {
                            Some("{") => depth += 1,
                            Some("}") => {
                                depth -= 1;
                                if depth == 0 {
                                    return Some(&tokens[start + 1..end]);
                                }
                            }
                            _ => {}
                        }
                    }
                    return None;
                }
                current += 1;
            }
        }
        None
    }

    /// Validates the document structure.
    ///
    /// Checks that all pointers reference valid records and that
    /// the topology is consistent.
    pub fn validate(&self) -> Vec<SatValidationError> {
        let mut errors = Vec::new();
        let max_index = self.records.len() as i32;

        for record in &self.records {
            // Check attribute pointer
            if !record.attribute.is_null() {
                if let Some(idx) = record.attribute.index() {
                    if idx as i32 >= max_index {
                        errors.push(SatValidationError::InvalidPointer {
                            record_index: record.index,
                            pointer_value: record.attribute.0,
                            context: "attribute".to_string(),
                        });
                    }
                }
            }

            // Check all token pointers
            for (i, token) in record.tokens.iter().enumerate() {
                if let SatToken::Pointer(p) = token {
                    if !p.is_null() {
                        if let Some(idx) = p.index() {
                            if idx as i32 >= max_index {
                                errors.push(SatValidationError::InvalidPointer {
                                    record_index: record.index,
                                    pointer_value: p.0,
                                    context: format!("token[{}]", i),
                                });
                            }
                        }
                    }
                }
            }
        }

        errors
    }

    /// Fill missing vertex-edge and edge-coedge links for ACIS serialization.
    /// Preserve existing non-null links.
    pub fn complete_brep_links(&mut self) {
        use std::collections::HashMap;

        let mut vertex_edges = HashMap::<i32, i32>::new();
        let mut edge_coedges = HashMap::<i32, i32>::new();

        for record in &self.records {
            if record.is_a("edge") {
                for token in [1, 3] {
                    if let Some(vertex) =
                        record.token_pointer(token).filter(|value| !value.is_null())
                    {
                        vertex_edges.entry(vertex.0).or_insert(record.index);
                    }
                }
            } else if record.is_a("coedge") {
                if let Some(edge) = record.token_pointer(4).filter(|value| !value.is_null()) {
                    edge_coedges.entry(edge.0).or_insert(record.index);
                }
            }
        }

        for record in &mut self.records {
            if record.is_a("vertex") {
                let missing_edge = record.token_pointer(1).is_none_or(|value| value.is_null());
                if missing_edge {
                    if let Some(&edge) = vertex_edges.get(&record.index) {
                        record.tokens[1] = SatToken::Pointer(SatPointer::new(edge));
                    }
                }
            } else if record.is_a("edge") {
                let missing_coedge = record.token_pointer(5).is_none_or(|value| value.is_null());
                if missing_coedge {
                    if let Some(&coedge) = edge_coedges.get(&record.index) {
                        record.tokens[5] = SatToken::Pointer(SatPointer::new(coedge));
                    }
                }
            }
        }
    }

    /// Strip non-geometry entities for SAB binary encoding.
    ///
    /// AutoCAD/IntelliCAD's ACIS SAB format only includes core geometric
    /// entities. Custom attribute entities (like `eye_refinement`,
    /// `vertex_template`, `*-attrib`) cause "NOT THAT KIND OF CLASS"
    /// errors in the ACIS kernel when encountered in SAB binary data.
    ///
    /// This method removes all non-core entities and remaps pointer
    /// references in the remaining records.
    pub fn strip_for_sab(&mut self) {
        // Determine which records to keep.
        // Core ACIS base types (last segment after hyphen split):
        let keep: Vec<bool> = self
            .records
            .iter()
            .map(|r| Self::is_core_geometry_type(&r.entity_type))
            .collect();

        let kept_count = keep.iter().filter(|&&k| k).count();
        if kept_count == self.records.len() {
            return; // nothing to strip
        }

        // Build old-index → new-index mapping.
        // Removed records map to -1 (null pointer).
        let mut index_map = vec![-1i32; self.records.len()];
        let mut new_idx: i32 = 0;
        for (old_idx, &kept) in keep.iter().enumerate() {
            if kept {
                index_map[old_idx] = new_idx;
                new_idx += 1;
            }
        }

        // Remap a single pointer value
        let remap = |p: i32| -> i32 {
            if p < 0 || (p as usize) >= index_map.len() {
                -1
            } else {
                index_map[p as usize]
            }
        };

        // Filter and remap records
        let mut new_records = Vec::with_capacity(kept_count);
        let old_records = std::mem::take(&mut self.records);
        for (old_idx, record) in old_records.into_iter().enumerate() {
            if !keep[old_idx] {
                continue;
            }
            let mut rec = record;
            rec.index = index_map[old_idx];

            // Remap attribute pointer — always null since all attribs are stripped
            rec.attribute = SatPointer::new(remap(rec.attribute.0));

            // Remap all pointer tokens
            for token in &mut rec.tokens {
                if let SatToken::Pointer(p) = token {
                    p.0 = remap(p.0);
                }
            }

            new_records.push(rec);
        }

        self.records = new_records;
        self.header.num_records = self.records.len();

        // Normalize spatial_resolution to 1.0 for SAB output.
        // IntelliCAD/AutoCAD always use 1.0 in native SAB data.
        // Source files from older ACIS versions may use different values
        // (e.g. 10.0) which can cause compatibility issues.
        self.header.spatial_resolution = 1.0;
    }

    /// Check if an entity type is a core ACIS geometry type that should
    /// be preserved in SAB output.
    fn is_core_geometry_type(entity_type: &str) -> bool {
        // Get the base type (last segment after hyphen split)
        let base = if let Some(pos) = entity_type.rfind('-') {
            &entity_type[pos + 1..]
        } else {
            entity_type
        };

        matches!(
            base,
            "body"
                | "lump"
                | "shell"
                | "subshell"
                | "face"
                | "loop"
                | "coedge"
                | "edge"
                | "vertex"
                | "wire"
                | "point"
                | "curve"
                | "surface"
                | "pcurve"
                | "transform"
                | "asmheader"
        )
    }
}

impl Default for SatDocument {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Error during SAT parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum SatParseError {
    /// The input text is empty or too short.
    EmptyInput,
    /// Failed to parse the header line.
    InvalidHeader(String),
    /// Failed to parse the product info line.
    InvalidProductInfo(String),
    /// Failed to parse the tolerance line.
    InvalidTolerances(String),
    /// Failed to parse an entity record.
    InvalidRecord { line: usize, message: String },
    /// Unexpected token.
    UnexpectedToken { line: usize, token: String },
}

impl fmt::Display for SatParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "SAT data is empty"),
            Self::InvalidHeader(msg) => write!(f, "Invalid SAT header: {}", msg),
            Self::InvalidProductInfo(msg) => write!(f, "Invalid product info: {}", msg),
            Self::InvalidTolerances(msg) => write!(f, "Invalid tolerances: {}", msg),
            Self::InvalidRecord { line, message } => {
                write!(f, "Invalid SAT record at line {}: {}", line, message)
            }
            Self::UnexpectedToken { line, token } => {
                write!(f, "Unexpected token '{}' at line {}", token, line)
            }
        }
    }
}

impl std::error::Error for SatParseError {}

/// Validation error in a SAT document.
#[derive(Debug, Clone, PartialEq)]
pub enum SatValidationError {
    /// A pointer references a non-existent record.
    InvalidPointer {
        record_index: i32,
        pointer_value: i32,
        context: String,
    },
}

impl fmt::Display for SatValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPointer {
                record_index,
                pointer_value,
                context,
            } => write!(
                f,
                "Record {} has invalid pointer ${} in {}",
                record_index, pointer_value, context
            ),
        }
    }
}

// ============================================================================
// Helper: SAT Entity Type Classification
// ============================================================================

/// Classification of SAT entity types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SatEntityCategory {
    /// Assembly header (asmheader).
    Header,
    /// Topological entity (body, lump, shell, face, loop, coedge, edge, vertex).
    Topology,
    /// Geometric entity (point, curve, surface).
    Geometry,
    /// Transform entity.
    Transform,
    /// Attribute entity.
    Attribute,
    /// Unknown entity type.
    Unknown,
}

/// Classify a SAT entity type name.
pub fn classify_entity_type(entity_type: &str) -> SatEntityCategory {
    match entity_type {
        "asmheader" => SatEntityCategory::Header,
        "body" | "lump" | "shell" | "subshell" | "face" | "loop" | "coedge" | "edge" | "vertex"
        | "wire" => SatEntityCategory::Topology,
        "point" | "straight-curve" | "ellipse-curve" | "intcurve-curve" | "bs3-curve"
        | "pcurve" | "plane-surface" | "cone-surface" | "sphere-surface" | "torus-surface"
        | "spline-surface" | "meshsurf-surface" | "bs3-surface" => SatEntityCategory::Geometry,
        "transform" => SatEntityCategory::Transform,
        _ if matches!(
            base_entity_type(entity_type),
            "body"
                | "lump"
                | "shell"
                | "subshell"
                | "face"
                | "loop"
                | "coedge"
                | "edge"
                | "vertex"
                | "wire"
        ) =>
        {
            SatEntityCategory::Topology
        }
        _ if matches!(
            base_entity_type(entity_type),
            "point" | "curve" | "surface" | "pcurve"
        ) =>
        {
            SatEntityCategory::Geometry
        }
        _ if entity_type.ends_with("-attrib") || entity_type.starts_with("attrib") => {
            SatEntityCategory::Attribute
        }
        _ => SatEntityCategory::Unknown,
    }
}

/// Returns the base class from a hyphen-separated SAB entity type.
///
/// For example, `tcoedge-coedge` derives from `coedge`.
pub fn base_entity_type(entity_type: &str) -> &str {
    entity_type.rsplit('-').next().unwrap_or(entity_type)
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;

    #[test]
    fn sab_coordinate_tokens_use_the_legacy_position_json_shape() {
        let values = [8.863414495014042e-14, -1.0, 0.0];
        let data = values
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let token = SatToken::Sab { tag: 0x14, data };

        assert_eq!(
            serde_json::to_value(&token).unwrap(),
            serde_json::json!({ "Position": values })
        );
        assert_eq!(
            serde_json::from_value::<SatToken>(serde_json::json!({ "Position": values })).unwrap(),
            SatToken::Position(values[0], values[1], values[2])
        );
    }
}
