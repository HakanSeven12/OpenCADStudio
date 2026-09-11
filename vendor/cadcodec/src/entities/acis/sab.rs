//! SAB (ACIS Binary) format converter.
//!
//! Converts between SAT text format (used pre-AC1027) and SAB binary format
//! (used in AC1027 / R2013 and later). The SAB format stores the same ACIS
//! topology/geometry data but uses binary tags instead of text tokens.
//!
//! # SAB Tag Bytes
//!
//! | Tag  | Meaning           | Data               |
//! |------|-------------------|---------------------|
//! | 0x02 | Character value   | 1 byte signed       |
//! | 0x03 | Short value       | 2 bytes LE i16      |
//! | 0x04 | Integer value     | 4 bytes LE i32      |
//! | 0x05 | Float value       | 4 bytes IEEE        |
//! | 0x06 | Double value      | 8 bytes LE f64      |
//! | 0x07 | String literal    | 1-byte len + bytes  |
//! | 0x08 | String literal    | 2-byte len + bytes  |
//! | 0x09 | String literal    | 4-byte len + bytes  |
//! | 0x0A | False / Reversed  | (no data)           |
//! | 0x0B | True / Forward    | (no data)           |
//! | 0x0C | Entity pointer    | 4 bytes LE i32      |
//! | 0x0D | Entity type name  | 1-byte len + bytes  |
//! | 0x0E | Subtype prefix    | 1-byte len + bytes  |
//! | 0x11 | End of record     | (no data)           |
//! | 0x13 | Position (x,y,z)  | 24 bytes (3×f64 LE) |
//! | 0x14 | Direction (x,y,z) | 24 bytes (3×f64 LE) |
//! | 0x15 | Enum value        | 4 bytes LE i32      |
//! | 0x16 | U-V vector        | 16 bytes (2×f64 LE) |
//! | 0x17 | 64-bit integer    | 8 bytes LE i64      |

use super::types::*;

/// SAB binary tag constants.
pub mod tags {
    /// Signed character value.
    pub const CHARACTER: u8 = 0x02;
    /// Signed short value.
    pub const SHORT: u8 = 0x03;
    /// Integer value (plain int, not entity pointer).
    pub const INTEGER: u8 = 0x04;
    /// Single-precision float.
    pub const FLOAT: u8 = 0x05;
    /// Double-precision float.
    pub const DOUBLE: u8 = 0x06;
    /// String literal with length prefix.
    pub const STRING: u8 = 0x07;
    /// String literal with a 2-byte length prefix.
    pub const SHORT_STRING: u8 = 0x08;
    /// String literal with a 4-byte length prefix.
    pub const LONG_STRING: u8 = 0x09;
    /// Boolean false / reversed / double-sided.
    pub const FALSE: u8 = 0x0A;
    /// Boolean true / forward / single-sided.
    pub const TRUE: u8 = 0x0B;
    /// Entity pointer reference (like `$n` in SAT).
    pub const POINTER: u8 = 0x0C;
    /// Entity type name.
    pub const ENTITY_TYPE: u8 = 0x0D;
    /// Subtype prefix (for compound types like `plane-surface`).
    pub const SUBTYPE: u8 = 0x0E;
    /// Subtype block start (`{` in SAT) — opens a nested subrecord, e.g. the
    /// `skin_spl_sur` block inside a lofted spline-surface.
    pub const SUBTYPE_START: u8 = 0x0F;
    /// Subtype block end (`}` in SAT) — closes a nested subrecord.
    pub const SUBTYPE_END: u8 = 0x10;
    /// End of record marker.
    pub const END_OF_RECORD: u8 = 0x11;
    /// Newer ASM long string literal (4-byte u32 length prefix + bytes).
    pub const ASM_LONG_STRING: u8 = 0x12;
    /// Position (3 doubles: x, y, z).
    pub const POSITION: u8 = 0x13;
    /// Direction (3 doubles: x, y, z).
    pub const DIRECTION: u8 = 0x14;
    /// Enumerated value (4-byte int). Emitted by ASM / ShapeManager records
    /// (AutoCAD 2013+).
    pub const ENUM: u8 = 0x15;
    /// U-V vector (2 doubles).
    pub const UV: u8 = 0x16;
    /// Signed 64-bit integer used by newer ASM / ShapeManager records.
    pub const INTEGER64: u8 = 0x17;
}

/// SAB header magic string.
const SAB_MAGIC: &[u8] = b"ACIS BinaryFile";

// ============================================================================
// SAT → SAB Writer
// ============================================================================

/// Converts a [`SatDocument`] to SAB binary format.
pub struct SabWriter;

impl SabWriter {
    /// Convert a SAT document to SAB binary data.
    pub fn write(doc: &SatDocument) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8192);

        // Header
        Self::write_header(&mut buf, &doc.header);

        // Entity records
        for record in &doc.records {
            Self::write_record(&mut buf, record);
        }

        // End marker: entity type "End-of-ACIS-data" with no end-of-record tag
        Self::write_entity_type(&mut buf, "End-of-ACIS-data");

        buf
    }

    fn write_header(buf: &mut Vec<u8>, header: &SatHeader) {
        // Magic
        buf.extend_from_slice(SAB_MAGIC);

        // Version number (4 bytes LE)
        let ver = header.version.sat_version_number();
        buf.extend_from_slice(&ver.to_le_bytes());

        // num_records field (4 bytes LE) — always 0 for ACIS 7.0+
        let num_records: u32 = if header.version.has_explicit_indices() {
            0
        } else {
            header.num_records as u32
        };
        buf.extend_from_slice(&num_records.to_le_bytes());

        // num_bodies (4 bytes LE)
        buf.extend_from_slice(&(header.num_bodies as u32).to_le_bytes());

        // has_history (4 bytes LE)
        let history: u32 = if header.has_history { 1 } else { 0 };
        buf.extend_from_slice(&history.to_le_bytes());

        // Product info strings
        Self::write_string(buf, &header.product_id);
        Self::write_string(buf, &header.product_version);
        Self::write_string(buf, &header.date);

        // Tolerances
        Self::write_double(buf, header.spatial_resolution);
        Self::write_double(buf, header.normal_tolerance);
        if let Some(resfit) = header.resfit_tolerance {
            Self::write_double(buf, resfit);
        }
    }

    fn write_record(buf: &mut Vec<u8>, record: &SatRecord) {
        // Entity type — may be compound with multiple hyphens.
        // In SAB, each level of the class hierarchy is a separate tag:
        //   "plane-surface"               → 0x0E("plane") + 0x0D("surface")
        //   "fmesh-eye-attrib"            → 0x0E("fmesh") + 0x0E("eye") + 0x0D("attrib")
        //   "persubent-acadSolidHistory-attrib" → 0x0E("persubent") + 0x0E("acadSolidHistory") + 0x0D("attrib")
        // The last segment is always the base type (0x0D ENTITY_TYPE);
        // all preceding segments are subtype prefixes (0x0E SUBTYPE).
        if record.entity_type.contains('-') {
            let parts: Vec<&str> = record.entity_type.split('-').collect();
            // All parts except the last are subtypes
            for &part in &parts[..parts.len() - 1] {
                Self::write_subtype(buf, part);
            }
            // Last part is the base entity type
            Self::write_entity_type(buf, parts[parts.len() - 1]);
        } else {
            Self::write_entity_type(buf, &record.entity_type);
        }

        // Attribute pointer
        Self::write_pointer(buf, record.attribute.0);

        // Subtype ID (plain integer, not pointer)
        Self::write_integer(buf, record.subtype_id);

        // Remaining tokens — with entity-type-aware coordinate grouping.
        // In SAT text, coordinates are individual Float tokens, but SAB uses
        // composite position(0x13)/direction(0x14) tags for coordinate triplets.
        let layout = CoordLayout::for_entity(&record.entity_type);
        let ints_as_doubles = Self::integers_are_doubles(&record.entity_type);
        Self::write_tokens_with_coord_grouping(buf, &record.tokens, &layout, ints_as_doubles);

        // End of record
        buf.push(tags::END_OF_RECORD);
    }

    /// Write tokens with coordinate grouping based on entity type layout.
    ///
    /// For geometric entities (surfaces, curves, points), consecutive Float
    /// tokens that represent coordinates are grouped into Position/Direction
    /// composite tags. The layout describes the exact sequence of triplets
    /// and scalars for each entity type.
    fn write_tokens_with_coord_grouping(
        buf: &mut Vec<u8>,
        tokens: &[SatToken],
        layout: &CoordLayout,
        ints_as_doubles: bool,
    ) {
        let mut i = 0;
        let mut step_index = 0; // tracks position in layout.steps
        let mut subtype_depth = 0usize;
        let mut active_steps = layout.steps;

        // Skip the first Pointer token (v700 unknown/$-1) to count geometry tokens
        let geom_start = tokens.iter().position(|t| Self::is_numeric(t));

        while i < tokens.len() {
            // ACIS subtype names and geometry keywords use identifier tags,
            // not counted strings. Keep explicit String tokens as strings.
            if subtype_depth > 0 {
                if let SatToken::Ident(name) = &tokens[i] {
                    if name != "{" && name != "}" && Self::string_to_boolean(name).is_none() {
                        // Explicit UV curves and intersection curves embed
                        // analytic surface geometry inside their subtype.
                        // Its vectors need the same binary grouping as a
                        // standalone surface, without grouping UV controls.
                        let inline_layout = match name.as_str() {
                            "plane" | "cone" => Some(CoordLayout::POS_DIR_DIR),
                            "sphere" => Some(CoordLayout::POS_S_DIR_DIR),
                            "torus" => Some(CoordLayout::POS_DIR_SS_DIR),
                            _ => None,
                        };
                        if let Some(inline_layout) = inline_layout {
                            active_steps = inline_layout.steps;
                            step_index = 0;
                        }
                        Self::write_entity_type(buf, name);
                        i += 1;
                        continue;
                    }
                }
            }
            // A token decoded from SAB already carries its original tag and
            // payload. Re-emit it byte-for-byte instead of applying SAT
            // coordinate grouping or numeric coercion.
            if matches!(&tokens[i], SatToken::Sab { .. }) {
                if matches!(
                    &tokens[i],
                    SatToken::Sab {
                        tag: tags::POSITION | tags::DIRECTION,
                        ..
                    }
                ) {
                    step_index += 1;
                }
                Self::write_token(buf, &tokens[i], ints_as_doubles && subtype_depth == 0);
                i += 1;
                continue;
            }

            // Explicit semantic positions are already grouped.
            if matches!(&tokens[i], SatToken::Position(_, _, _)) {
                Self::write_token(buf, &tokens[i], ints_as_doubles && subtype_depth == 0);
                step_index += 1;
                i += 1;
                continue;
            }

            // Are we in the geometry section of the token stream?
            let in_geom = geom_start.map(|gs| i >= gs).unwrap_or(false);

            if in_geom && step_index < active_steps.len() {
                match active_steps[step_index] {
                    Some(tag) => {
                        // This step expects a coordinate triplet (3 floats → Position/Direction)
                        if i + 2 < tokens.len()
                            && Self::is_numeric(&tokens[i])
                            && Self::is_numeric(&tokens[i + 1])
                            && Self::is_numeric(&tokens[i + 2])
                        {
                            let x = Self::numeric_value(&tokens[i]);
                            let y = Self::numeric_value(&tokens[i + 1]);
                            let z = Self::numeric_value(&tokens[i + 2]);

                            buf.push(tag);
                            buf.extend_from_slice(&x.to_le_bytes());
                            buf.extend_from_slice(&y.to_le_bytes());
                            buf.extend_from_slice(&z.to_le_bytes());

                            i += 3;
                            step_index += 1;
                        } else {
                            // Not enough numeric tokens for a triplet — write individually
                            Self::write_token(
                                buf,
                                &tokens[i],
                                ints_as_doubles && subtype_depth == 0,
                            );
                            i += 1;
                        }
                    }
                    None => {
                        // This step expects a scalar double (single float value)
                        Self::write_token(buf, &tokens[i], ints_as_doubles && subtype_depth == 0);
                        i += 1;
                        step_index += 1;
                    }
                }
            } else {
                Self::write_token(buf, &tokens[i], ints_as_doubles && subtype_depth == 0);
                i += 1;
            }

            match tokens[i - 1].as_ident() {
                Some("{") => subtype_depth += 1,
                Some("}") => subtype_depth = subtype_depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    /// Check if a token is a numeric value (Float or Integer).
    fn is_numeric(token: &SatToken) -> bool {
        matches!(token, SatToken::Float(_) | SatToken::Integer(_))
    }

    /// Extract numeric value from a Float or Integer token.
    fn numeric_value(token: &SatToken) -> f64 {
        match token {
            SatToken::Float(v) => *v,
            SatToken::Integer(v) => *v as f64,
            _ => 0.0,
        }
    }

    fn write_token(buf: &mut Vec<u8>, token: &SatToken, ints_as_doubles: bool) {
        match token {
            SatToken::Pointer(p) => Self::write_pointer(buf, p.0),
            // Direct geometry parameters such as edge start/end values are
            // doubles. NURBS subtype fields are routed here with `false` so
            // their degree and multiplicities stay integer-tagged in SAB.
            SatToken::Integer(v) => {
                if ints_as_doubles {
                    Self::write_double(buf, *v as f64);
                } else {
                    Self::write_integer(buf, *v as i32);
                }
            }
            SatToken::Float(v) => Self::write_double(buf, *v),
            // String tokens from @-counted SAT format may be boolean keywords
            // (e.g., @9 reverse_v, @9 forward_v). Map them to TRUE/FALSE tags.
            SatToken::String(s) => {
                if let Some(val) = Self::string_to_boolean(s) {
                    buf.push(if val { tags::TRUE } else { tags::FALSE });
                } else {
                    Self::write_string(buf, s);
                }
            }
            SatToken::Position(x, y, z) => Self::write_position(buf, *x, *y, *z),
            SatToken::True => buf.push(tags::TRUE),
            SatToken::False => buf.push(tags::FALSE),
            SatToken::Terminator => buf.push(tags::END_OF_RECORD),
            SatToken::Ident(s) => Self::write_ident_token(buf, s),
            SatToken::Enum(s) => Self::write_enum_token(buf, s),
            SatToken::Sab { tag, data } => {
                buf.push(*tag);
                buf.extend_from_slice(data);
            }
        }
    }

    /// Check if a string value is a known ACIS boolean keyword.
    /// Returns `Some(true)` for forward/positive, `Some(false)` for reversed/negative.
    fn string_to_boolean(s: &str) -> Option<bool> {
        match s {
            "forward_v" | "I" | "forward" | "single" | "in" => Some(true),
            "reverse_v" | "reversed_v" | "reversed" | "double" | "out" | "F" => Some(false),
            _ => None,
        }
    }

    fn write_ident_token(buf: &mut Vec<u8>, ident: &str) {
        match ident {
            "{" => buf.push(tags::SUBTYPE_START),
            "}" => buf.push(tags::SUBTYPE_END),
            _ => {
                // Map known boolean identifiers to True/False tags
                if let Some(val) = Self::string_to_boolean(ident) {
                    buf.push(if val { tags::TRUE } else { tags::FALSE });
                } else {
                    Self::write_string(buf, ident);
                }
            }
        }
    }

    fn write_enum_token(buf: &mut Vec<u8>, name: &str) {
        match name {
            "full" | "open" | "none" | "closed" | "periodic" => {
                let value: i32 = match name {
                    "closed" => 1,
                    "periodic" => 2,
                    _ => 0,
                };
                buf.push(tags::ENUM);
                buf.extend_from_slice(&value.to_le_bytes());
            }
            "forward" | "single" | "in" => buf.push(tags::TRUE),
            "reversed" | "double" | "out" => buf.push(tags::FALSE),
            // "unknown" and other enum values → string
            _ => Self::write_string(buf, name),
        }
    }

    fn write_entity_type(buf: &mut Vec<u8>, name: &str) {
        buf.push(tags::ENTITY_TYPE);
        buf.push(name.len() as u8);
        buf.extend_from_slice(name.as_bytes());
    }

    /// Determine whether direct geometry integer literals are scalar doubles.
    ///
    /// NURBS subtype blocks are handled separately in
    /// [`Self::write_tokens_with_coord_grouping`]: their degree, knot counts,
    /// and knot multiplicities are SAB integers even when the enclosing
    /// entity is a curve, surface, or pcurve.
    fn integers_are_doubles(entity_type: &str) -> bool {
        matches!(
            entity_type,
            "point"
                | "straight-curve"
                | "ellipse-curve"
                | "plane-surface"
                | "cone-surface"
                | "sphere-surface"
                | "torus-surface"
                | "edge"
        )
    }

    fn write_subtype(buf: &mut Vec<u8>, name: &str) {
        buf.push(tags::SUBTYPE);
        buf.push(name.len() as u8);
        buf.extend_from_slice(name.as_bytes());
    }

    fn write_pointer(buf: &mut Vec<u8>, value: i32) {
        buf.push(tags::POINTER);
        buf.extend_from_slice(&value.to_le_bytes());
    }

    fn write_integer(buf: &mut Vec<u8>, value: i32) {
        buf.push(tags::INTEGER);
        buf.extend_from_slice(&value.to_le_bytes());
    }

    fn write_double(buf: &mut Vec<u8>, value: f64) {
        buf.push(tags::DOUBLE);
        buf.extend_from_slice(&value.to_le_bytes());
    }

    fn write_string(buf: &mut Vec<u8>, s: &str) {
        if s.len() > u16::MAX as usize {
            buf.push(tags::LONG_STRING);
            buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
        } else if s.len() > u8::MAX as usize {
            buf.push(tags::SHORT_STRING);
            buf.extend_from_slice(&(s.len() as u16).to_le_bytes());
        } else {
            buf.push(tags::STRING);
            buf.push(s.len() as u8);
        }
        buf.extend_from_slice(s.as_bytes());
    }

    fn write_position(buf: &mut Vec<u8>, x: f64, y: f64, z: f64) {
        buf.push(tags::POSITION);
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf.extend_from_slice(&z.to_le_bytes());
    }

    #[allow(dead_code)]
    fn write_direction(buf: &mut Vec<u8>, x: f64, y: f64, z: f64) {
        buf.push(tags::DIRECTION);
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf.extend_from_slice(&z.to_le_bytes());
    }
}

// ============================================================================
// Coordinate layout for entity-type-aware SAB encoding
// ============================================================================

/// Describes the layout of coordinate triplets and scalars in a geometry record.
///
/// In SAT text, positions and directions are both written as three individual floats.
/// In SAB binary, positions use tag `0x13` and directions use tag `0x14`, each
/// encoding three f64 values as a single composite token.
///
/// Some entities (like `sphere-surface` and `torus-surface`) have scalar float
/// values interleaved between coordinate triplets. The layout must describe
/// the exact sequence to group correctly.
struct CoordLayout {
    /// Sequence of geometry tokens after the initial v700 $-1 pointer.
    ///
    /// - `Some(tag)` = group next 3 floats as a composite Position/Direction.
    /// - `None` = write next float as an individual DOUBLE scalar.
    steps: &'static [Option<u8>],
}

impl CoordLayout {
    const EMPTY: Self = Self { steps: &[] };

    /// Position only (e.g., `point`)
    const POS: Self = Self {
        steps: &[Some(tags::POSITION)],
    };

    /// Position + direction (e.g., `straight-curve`)
    const POS_DIR: Self = Self {
        steps: &[Some(tags::POSITION), Some(tags::DIRECTION)],
    };

    /// Position + direction + direction (e.g., `plane-surface`)
    const POS_DIR_DIR: Self = Self {
        steps: &[
            Some(tags::POSITION),
            Some(tags::DIRECTION),
            Some(tags::DIRECTION),
        ],
    };

    /// Position + direction + position (e.g., future entity types where
    /// the 3rd triplet's magnitude carries meaning).
    #[allow(dead_code)]
    const POS_DIR_POS: Self = Self {
        steps: &[
            Some(tags::POSITION),
            Some(tags::DIRECTION),
            Some(tags::POSITION),
        ],
    };

    /// Position + scalar + direction + direction (e.g., `sphere-surface`)
    ///
    /// sphere-surface: center(pos) radius(scalar) u_dir(dir) pole(dir)
    const POS_S_DIR_DIR: Self = Self {
        steps: &[
            Some(tags::POSITION),
            None, // radius (scalar double)
            Some(tags::DIRECTION),
            Some(tags::DIRECTION),
        ],
    };

    /// Position + direction + scalar + scalar + direction (e.g., `torus-surface`)
    ///
    /// torus-surface: center(pos) normal(dir) major_r(scalar) minor_r(scalar) u_dir(dir)
    const POS_DIR_SS_DIR: Self = Self {
        steps: &[
            Some(tags::POSITION),
            Some(tags::DIRECTION),
            None, // major_radius (scalar)
            None, // minor_radius (scalar)
            Some(tags::DIRECTION),
        ],
    };

    /// Determine the coordinate layout for a given entity type.
    fn for_entity(entity_type: &str) -> Self {
        match entity_type {
            "point" => Self::POS,
            "straight-curve" => Self::POS_DIR,
            "plane-surface" => Self::POS_DIR_DIR,
            "cone-surface" => Self::POS_DIR_DIR,
            "sphere-surface" => Self::POS_S_DIR_DIR,
            "torus-surface" => Self::POS_DIR_SS_DIR,
            "ellipse-curve" => Self::POS_DIR_DIR,
            _ => Self::EMPTY,
        }
    }
}

// ============================================================================
// SAB → SAT Reader
// ============================================================================

/// Error type for SAB parsing.
#[derive(Debug)]
pub enum SabError {
    /// Unexpected end of data.
    UnexpectedEof,
    /// Invalid magic header.
    InvalidMagic,
    /// Unknown tag byte.
    UnknownTag(u8, usize),
    /// Invalid string encoding.
    InvalidString,
}

impl std::fmt::Display for SabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SabError::UnexpectedEof => write!(f, "Unexpected end of SAB data"),
            SabError::InvalidMagic => write!(f, "Invalid SAB magic header"),
            SabError::UnknownTag(tag, pos) => {
                write!(f, "Unknown SAB tag 0x{:02X} at position {}", tag, pos)
            }
            SabError::InvalidString => write!(f, "Invalid UTF-8 string in SAB data"),
        }
    }
}

impl std::error::Error for SabError {}

/// Reads SAB binary data and produces a [`SatDocument`].
pub struct SabReader;

impl SabReader {
    /// Parse SAB binary data into a SAT document.
    pub fn read(data: &[u8]) -> Result<SatDocument, SabError> {
        Self::read_with_consumed(data).map(|(doc, _)| doc)
    }

    /// Like [`read`](Self::read), also returning how many input bytes the
    /// payload occupied (through the End-of-ACIS-data record). R2004–R2006
    /// DWGs embed the SAB with no length prefix, so the caller measures the
    /// blob by parsing it.
    pub fn read_with_consumed(data: &[u8]) -> Result<(SatDocument, usize), SabError> {
        // Two header magics: classic ACIS ("ACIS BinaryFile", 15 bytes) and the
        // newer Autodesk ShapeManager ("ASM BinaryFile", 14 bytes) emitted by
        // AutoCAD 2013+ and carried in the AcDs data store. In both the version
        // u32 begins 15 bytes in: ACIS's magic is 15 bytes; ASM's 14-byte magic
        // is followed by one trailing byte before the header ints.
        const SAB_MAGIC_ASM: &[u8] = b"ASM BinaryFile";
        let mut pos = if data.starts_with(SAB_MAGIC) {
            SAB_MAGIC.len()
        } else if data.starts_with(SAB_MAGIC_ASM) {
            SAB_MAGIC.len() // 15 = 14-byte ASM magic + 1 trailing byte
        } else {
            return Err(SabError::InvalidMagic);
        };

        // Header ints (4 × u32 LE)
        let version_num = read_u32(data, &mut pos)?;
        let num_records = read_u32(data, &mut pos)? as usize;
        let num_bodies = read_u32(data, &mut pos)? as usize;
        let has_history = read_u32(data, &mut pos)? != 0;

        let version = SatVersion::from_sat_number(version_num);

        // Header strings (3 tagged strings)
        let product_id = read_tagged_string(data, &mut pos)?;
        let product_version = read_tagged_string(data, &mut pos)?;
        let date = read_tagged_string(data, &mut pos)?;

        // Tolerances (2 or 3 tagged doubles)
        let spatial_resolution = read_tagged_double(data, &mut pos)?;
        let normal_tolerance = read_tagged_double(data, &mut pos)?;

        // Third tolerance (resfit) — officially ACIS 7.0+ only, but some
        // writers (e.g. Open Design Alliance ACIS Builder) include it in
        // older versions too.  Peek at the next byte: if it's a DOUBLE
        // tag we read it; otherwise skip.
        let resfit_tolerance = if pos < data.len() && data[pos] == tags::DOUBLE {
            Some(read_tagged_double(data, &mut pos)?)
        } else {
            None
        };

        let header = SatHeader {
            version,
            num_records,
            num_bodies,
            has_history,
            product_id,
            product_version,
            date,
            spatial_resolution,
            normal_tolerance,
            resfit_tolerance,
        };

        // Parse entity records
        let mut records = Vec::new();
        let mut record_index: i32 = 0;

        while pos < data.len() {
            let tag = data[pos];
            if tag == tags::ENTITY_TYPE || tag == tags::SUBTYPE {
                let (record, new_pos) = Self::read_record(data, pos, record_index)?;

                // Check for ACIS/ASM end marker — consume it so the
                // reported length covers the full payload.
                if matches!(
                    record.entity_type.as_str(),
                    "End-of-ACIS-data" | "End-of-ASM-data"
                ) {
                    pos = new_pos;
                    break;
                }

                pos = new_pos;
                records.push(record);
                record_index += 1;
            } else {
                return Err(SabError::UnknownTag(tag, pos));
            }
        }

        // Normalize pre-7.0 SAB records to v700 token layout by inserting
        // synthetic sentinel pointers, just like the SAT parser does.
        // SAB v600 records have the same layout as v400 SAT text (no
        // sentinel pointers).  The SatWriter's skip_index logic expects
        // the v700 layout, so we must add sentinels here.
        if version.major < 7 {
            for record in &mut records {
                super::parser::normalize_v400_tokens(&record.entity_type, &mut record.tokens);
            }
        }

        // Convert SAB boolean tags (TRUE/FALSE) to ACIS SAT keywords.
        // SAB uses binary TRUE(0x0B)/FALSE(0x0A) tags, but SAT text
        // uses context-dependent keywords:
        //   face:    sense (forward/reversed), side (single/double)
        //   coedge:  sense (forward/reversed)
        //   edge:    sense (forward/reversed)
        //   surface: sense (forward_v/reversed_v), bounds (I/F)
        for record in &mut records {
            convert_sab_booleans(&record.entity_type, &mut record.tokens);
        }

        Ok((SatDocument { header, records }, pos))
    }

    fn read_record(
        data: &[u8],
        mut pos: usize,
        index: i32,
    ) -> Result<(SatRecord, usize), SabError> {
        // Read entity type — may have multiple subtype prefixes.
        // In SAB, compound types like "fmesh-eye-attrib" are encoded as:
        //   0x0E("fmesh") + 0x0E("eye") + 0x0D("attrib")
        // We collect all 0x0E subtypes, then read the final 0x0D base type,
        // and join them with hyphens to reconstruct the SAT entity type name.
        let mut subtype_parts: Vec<String> = Vec::new();
        let entity_type;

        // Collect all subtype prefixes (0x0E)
        while pos < data.len() && data[pos] == tags::SUBTYPE {
            pos += 1;
            let (sub, new_pos) = read_length_string(data, pos)?;
            subtype_parts.push(sub);
            pos = new_pos;
        }

        // Read the base entity type (0x0D)
        if pos >= data.len() || data[pos] != tags::ENTITY_TYPE {
            return Err(SabError::UnknownTag(
                if pos < data.len() { data[pos] } else { 0 },
                pos,
            ));
        }
        pos += 1;
        let (base_type, new_pos) = read_length_string(data, pos)?;
        pos = new_pos;

        // Reconstruct compound name: subtypes joined with hyphens + base type
        if subtype_parts.is_empty() {
            entity_type = base_type;
        } else {
            subtype_parts.push(base_type);
            entity_type = subtype_parts.join("-");
        }

        let subtype_name = if entity_type.contains('-') {
            Some(entity_type.split('-').next().unwrap().to_string())
        } else {
            None
        };

        // Check for ACIS/ASM end marker (no record body)
        if matches!(entity_type.as_str(), "End-of-ACIS-data" | "End-of-ASM-data") {
            return Ok((
                SatRecord {
                    index,
                    entity_type,
                    sub_type: subtype_name,
                    attribute: SatPointer::NULL,
                    subtype_id: -1,
                    tokens: Vec::new(),
                    raw_text: None,
                },
                pos,
            ));
        }

        // Attribute pointer
        let attribute = if pos < data.len() && data[pos] == tags::POINTER {
            pos += 1;
            let val = read_i32(data, &mut pos)?;
            SatPointer::new(val)
        } else {
            SatPointer::NULL
        };

        // Subtype ID (plain integer)
        let subtype_id = if pos < data.len() && data[pos] == tags::INTEGER {
            pos += 1;
            read_i32(data, &mut pos)?
        } else {
            -1
        };

        // Remaining tokens until END_OF_RECORD
        let mut tokens = Vec::new();
        while pos < data.len() {
            let tag = data[pos];
            if tag == tags::END_OF_RECORD {
                pos += 1;
                break;
            }
            let (token, new_pos) = Self::read_token(data, pos)?;
            tokens.push(token);
            pos = new_pos;
        }

        Ok((
            SatRecord {
                index,
                entity_type,
                sub_type: subtype_name,
                attribute,
                subtype_id,
                tokens,
                raw_text: None,
            },
            pos,
        ))
    }

    fn read_token(data: &[u8], pos: usize) -> Result<(SatToken, usize), SabError> {
        if pos >= data.len() {
            return Err(SabError::UnexpectedEof);
        }

        let tag = data[pos];
        let mut pos = pos + 1;

        match tag {
            tags::POINTER => {
                let val = read_i32(data, &mut pos)?;
                Ok((SatToken::Pointer(SatPointer::new(val)), pos))
            }
            tags::TRUE => Ok((SatToken::True, pos)),
            tags::FALSE => Ok((SatToken::False, pos)),
            tags::END_OF_RECORD => Ok((SatToken::Terminator, pos)),
            tags::CHARACTER => Self::read_raw_fixed(data, tag, pos, 1),
            tags::SHORT => Self::read_raw_fixed(data, tag, pos, 2),
            tags::INTEGER | tags::FLOAT | tags::ENUM => Self::read_raw_fixed(data, tag, pos, 4),
            tags::DOUBLE | tags::INTEGER64 => Self::read_raw_fixed(data, tag, pos, 8),
            tags::POSITION | tags::DIRECTION => Self::read_raw_fixed(data, tag, pos, 24),
            tags::UV => Self::read_raw_fixed(data, tag, pos, 16),
            tags::STRING | tags::ENTITY_TYPE | tags::SUBTYPE => {
                Self::read_raw_string(data, tag, pos, 1)
            }
            tags::SHORT_STRING => Self::read_raw_string(data, tag, pos, 2),
            tags::LONG_STRING | tags::ASM_LONG_STRING => Self::read_raw_string(data, tag, pos, 4),
            // Keep nested subtype delimiters as their original zero-payload
            // SAB tags. `SatToken::as_ident` exposes them as `{` and `}` to
            // geometry consumers without losing binary identity.
            tags::SUBTYPE_START | tags::SUBTYPE_END => Ok((
                SatToken::Sab {
                    tag,
                    data: Vec::new(),
                },
                pos,
            )),
            _ => Err(SabError::UnknownTag(tag, pos - 1)),
        }
    }

    fn read_raw_fixed(
        data: &[u8],
        tag: u8,
        pos: usize,
        len: usize,
    ) -> Result<(SatToken, usize), SabError> {
        let end = pos.checked_add(len).ok_or(SabError::UnexpectedEof)?;
        if end > data.len() {
            return Err(SabError::UnexpectedEof);
        }
        Ok((
            SatToken::Sab {
                tag,
                data: data[pos..end].to_vec(),
            },
            end,
        ))
    }

    fn read_raw_string(
        data: &[u8],
        tag: u8,
        pos: usize,
        prefix_len: usize,
    ) -> Result<(SatToken, usize), SabError> {
        let prefix_end = pos.checked_add(prefix_len).ok_or(SabError::UnexpectedEof)?;
        if prefix_end > data.len() {
            return Err(SabError::UnexpectedEof);
        }
        let len = match prefix_len {
            1 => data[pos] as usize,
            2 => u16::from_le_bytes([data[pos], data[pos + 1]]) as usize,
            4 => u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                as usize,
            _ => return Err(SabError::UnexpectedEof),
        };
        let end = prefix_end.checked_add(len).ok_or(SabError::UnexpectedEof)?;
        if end > data.len() {
            return Err(SabError::UnexpectedEof);
        }
        Ok((
            SatToken::Sab {
                tag,
                data: data[pos..end].to_vec(),
            },
            end,
        ))
    }
}

// ============================================================================
// SAB boolean → SAT keyword conversion
// ============================================================================

/// Convert SAB TRUE/FALSE tokens to ACIS SAT keywords based on entity context.
///
/// SAB binary uses generic TRUE(0x0B)/FALSE(0x0A) tags for all boolean fields.
/// SAT text uses context-dependent keywords:
///   - face sense: `forward` / `reversed`
///   - face side: `single` / `double`
///   - coedge sense: `forward` / `reversed`
///   - edge sense: `forward` / `reversed`
///   - surface sense: `forward_v` / `reversed_v`
///   - surface bounds: `I` (infinite) / `F` (finite)
fn convert_sab_booleans(entity_type: &str, tokens: &mut Vec<SatToken>) {
    match base_entity_type(entity_type) {
        "face" => {
            // face: ... sense side #
            // After v700 normalization: tok[0]=sentinel, tok[1..N-2]=ptrs,
            // tok[N-2]=sense, tok[N-1]=side
            let len = tokens.len();
            if len >= 2 {
                // sense: True=forward, False=reversed
                if matches!(tokens[len - 2], SatToken::True | SatToken::False) {
                    let is_forward = matches!(tokens[len - 2], SatToken::True);
                    tokens[len - 2] =
                        SatToken::Enum(if is_forward { "forward" } else { "reversed" }.to_string());
                }
                // side: True=single, False=double
                if matches!(tokens[len - 1], SatToken::True | SatToken::False) {
                    let is_single = matches!(tokens[len - 1], SatToken::True);
                    tokens[len - 1] =
                        SatToken::Enum(if is_single { "single" } else { "double" }.to_string());
                }
            }
        }
        "coedge" => {
            // coedge: ... $edge sense $loop $pcurve #
            // Find the first True/False token and convert to sense
            for token in tokens.iter_mut() {
                if matches!(token, SatToken::True | SatToken::False) {
                    let is_forward = matches!(token, SatToken::True);
                    *token =
                        SatToken::Enum(if is_forward { "forward" } else { "reversed" }.to_string());
                    break; // only the first boolean is sense
                }
            }
        }
        "edge" => {
            // edge: ... $coedge $curve sense convexity unknown #
            // Find True/False tokens: first is sense
            for token in tokens.iter_mut() {
                if matches!(token, SatToken::True | SatToken::False) {
                    let is_forward = matches!(token, SatToken::True);
                    *token =
                        SatToken::Enum(if is_forward { "forward" } else { "reversed" }.to_string());
                    break;
                }
            }
        }
        _ if entity_type == "cone-surface" => {
            // cone-surface in v400: bool layout is position-dependent.
            // SAB stores booleans at the same structural positions as SAT, but
            // v600 and v400 interpret them differently:
            //   bool 0,1 → bounds (I/F)
            //   bool 2   → sense (forward/reversed)
            //   bool 3+  → bounds (I/F)
            let mut bool_idx = 0u32;
            for token in tokens.iter_mut() {
                if matches!(token, SatToken::True | SatToken::False) {
                    let is_true = matches!(token, SatToken::True);
                    *token = if bool_idx == 2 {
                        SatToken::Enum(if is_true { "forward" } else { "reversed" }.to_string())
                    } else {
                        SatToken::Enum(if is_true { "I" } else { "F" }.to_string())
                    };
                    bool_idx += 1;
                }
            }
        }
        _ if entity_type.ends_with("-surface") => {
            // Generic surface: first bool = sense (forward_v/reverse_v),
            // remaining bools = bound infinity (I/F).
            // Note: v400 ACIS uses "reverse_v" (not "reversed_v").
            let mut first = true;
            for token in tokens.iter_mut() {
                if matches!(token, SatToken::True | SatToken::False) {
                    if first {
                        let is_forward = matches!(token, SatToken::True);
                        *token = SatToken::Enum(
                            if is_forward { "forward_v" } else { "reverse_v" }.to_string(),
                        );
                        first = false;
                    } else {
                        let is_infinite = matches!(token, SatToken::True);
                        *token = SatToken::Enum(if is_infinite { "I" } else { "F" }.to_string());
                    }
                }
            }
        }
        _ if entity_type.ends_with("-curve") => {
            // Curve entities have varying boolean layouts:
            //
            // straight-curve, ellipse-curve: all booleans are bounds (I/F).
            //   In v400, ellipse-curve has no sense — just 2 bounds.
            //
            // intcurve-curve, bs2-curve, bs3-curve: first bool = sense, rest = bounds.
            let has_sense = entity_type != "straight-curve" && entity_type != "ellipse-curve";
            let mut found_sense = false;
            for token in tokens.iter_mut() {
                if matches!(token, SatToken::True | SatToken::False) {
                    if has_sense && !found_sense {
                        // First boolean is sense for curves that have it
                        let is_forward = matches!(token, SatToken::True);
                        *token = SatToken::Enum(
                            if is_forward { "forward" } else { "reversed" }.to_string(),
                        );
                        found_sense = true;
                    } else {
                        // Bound: True=Infinite(I), False=Finite(F)
                        let is_infinite = matches!(token, SatToken::True);
                        *token = SatToken::Enum(if is_infinite { "I" } else { "F" }.to_string());
                    }
                }
            }
        }
        _ => {}
    }
}

// ============================================================================
// Binary reading helpers
// ============================================================================

fn read_u32(data: &[u8], pos: &mut usize) -> Result<u32, SabError> {
    if *pos + 4 > data.len() {
        return Err(SabError::UnexpectedEof);
    }
    let val = u32::from_le_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    Ok(val)
}

fn read_i32(data: &[u8], pos: &mut usize) -> Result<i32, SabError> {
    if *pos + 4 > data.len() {
        return Err(SabError::UnexpectedEof);
    }
    let val = i32::from_le_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    Ok(val)
}

fn read_f64(data: &[u8], pos: &mut usize) -> Result<f64, SabError> {
    if *pos + 8 > data.len() {
        return Err(SabError::UnexpectedEof);
    }
    let val = f64::from_le_bytes([
        data[*pos],
        data[*pos + 1],
        data[*pos + 2],
        data[*pos + 3],
        data[*pos + 4],
        data[*pos + 5],
        data[*pos + 6],
        data[*pos + 7],
    ]);
    *pos += 8;
    Ok(val)
}

fn read_length_string(data: &[u8], pos: usize) -> Result<(String, usize), SabError> {
    if pos >= data.len() {
        return Err(SabError::UnexpectedEof);
    }
    let len = data[pos] as usize;
    let start = pos + 1;
    if start + len > data.len() {
        return Err(SabError::UnexpectedEof);
    }
    let s = std::str::from_utf8(&data[start..start + len])
        .map_err(|_| SabError::InvalidString)?
        .to_string();
    Ok((s, start + len))
}

fn read_tagged_string(data: &[u8], pos: &mut usize) -> Result<String, SabError> {
    if *pos >= data.len() {
        return Err(SabError::UnknownTag(0, *pos));
    }
    let tag = data[*pos];
    *pos += 1;
    let prefix_len = match tag {
        tags::STRING => 1,
        tags::SHORT_STRING => 2,
        tags::LONG_STRING | tags::ASM_LONG_STRING => 4,
        _ => return Err(SabError::UnknownTag(tag, *pos - 1)),
    };
    let prefix_end = (*pos)
        .checked_add(prefix_len)
        .ok_or(SabError::UnexpectedEof)?;
    if prefix_end > data.len() {
        return Err(SabError::UnexpectedEof);
    }
    let len = match prefix_len {
        1 => data[*pos] as usize,
        2 => u16::from_le_bytes([data[*pos], data[*pos + 1]]) as usize,
        4 => u32::from_le_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]])
            as usize,
        _ => return Err(SabError::UnexpectedEof),
    };
    let end = prefix_end.checked_add(len).ok_or(SabError::UnexpectedEof)?;
    if end > data.len() {
        return Err(SabError::UnexpectedEof);
    }
    let value = std::str::from_utf8(&data[prefix_end..end])
        .map_err(|_| SabError::InvalidString)?
        .to_string();
    *pos = end;
    Ok(value)
}

fn read_tagged_double(data: &[u8], pos: &mut usize) -> Result<f64, SabError> {
    if *pos >= data.len() || data[*pos] != tags::DOUBLE {
        return Err(SabError::UnknownTag(
            if *pos < data.len() { data[*pos] } else { 0 },
            *pos,
        ));
    }
    *pos += 1;
    read_f64(data, pos)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sat_to_sab_header() {
        let mut doc = SatDocument::new();
        doc.header.product_id = "TestApp".to_string();
        doc.header.product_version = "ACIS 7.0".to_string();
        doc.header.date = "Thu Jan 01 00:00:00 2023".to_string();
        doc.header.spatial_resolution = 10.0;
        doc.header.normal_tolerance = 1e-06;
        doc.header.resfit_tolerance = Some(1e-10);

        let sab = SabWriter::write(&doc);

        // Check magic
        assert_eq!(&sab[..15], b"ACIS BinaryFile");
        // Check version
        let ver = u32::from_le_bytes([sab[15], sab[16], sab[17], sab[18]]);
        assert_eq!(ver, 700);
        // Check End-of-ACIS-data is present
        let end_str = b"End-of-ACIS-data";
        assert!(sab.windows(end_str.len()).any(|w| w == end_str));
    }

    #[test]
    fn test_sat_to_sab_roundtrip() {
        let sat_text = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            10 9.9999999999999995e-007 1e-010\n\
            body $-1 -1 $-1 $1 $-1 $-1 #\n\
            lump $-1 -1 $-1 $-1 $2 $0 #\n\
            shell $-1 -1 $-1 $-1 $-1 $3 $-1 $1 #\n\
            face $-1 -1 $-1 $-1 $-1 $2 $-1 $4 forward single #\n\
            plane-surface $-1 -1 $-1 0 0 5 0 0 1 1 0 0 forward_v I I I I #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat_text).unwrap();
        let sab = SabWriter::write(&doc);
        let roundtrip = SabReader::read(&sab).unwrap();

        assert_eq!(roundtrip.header.version, doc.header.version);
        assert_eq!(roundtrip.records.len(), doc.records.len());
        assert_eq!(roundtrip.records[0].entity_type, "body");
        assert_eq!(roundtrip.records[3].entity_type, "face");
        assert_eq!(roundtrip.records[4].entity_type, "plane-surface");
    }

    #[test]
    fn test_compound_entity_types() {
        let sat_text = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            10 9.9999999999999995e-007 1e-010\n\
            plane-surface $-1 -1 $-1 0 0 5 0 0 1 1 0 0 forward_v I I I I #\n\
            straight-curve $-1 -1 $-1 0 0 0 1 0 0 I I #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat_text).unwrap();
        let sab = SabWriter::write(&doc);

        // Verify subtype tags are present
        assert!(sab.contains(&tags::SUBTYPE));

        // Roundtrip
        let roundtrip = SabReader::read(&sab).unwrap();
        assert_eq!(roundtrip.records[0].entity_type, "plane-surface");
        assert_eq!(roundtrip.records[1].entity_type, "straight-curve");
    }

    #[test]
    fn test_sab_boolean_mapping() {
        let sat_text = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            10 9.9999999999999995e-007 1e-010\n\
            face $-1 -1 $-1 $-1 $-1 $-1 $-1 $-1 forward single #\n\
            face $-1 -1 $-1 $-1 $-1 $-1 $-1 $-1 reversed double #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat_text).unwrap();
        let sab = SabWriter::write(&doc);
        let roundtrip = SabReader::read(&sab).unwrap();

        // forward/single → Enum("forward"), Enum("single")
        let face1 = &roundtrip.records[0];
        let last_two: Vec<_> = face1.tokens.iter().rev().take(2).collect();
        assert_eq!(last_two[0], &SatToken::Enum("single".to_string()));
        assert_eq!(last_two[1], &SatToken::Enum("forward".to_string()));

        // reversed/double → Enum("reversed"), Enum("double")
        let face2 = &roundtrip.records[1];
        let last_two: Vec<_> = face2.tokens.iter().rev().take(2).collect();
        assert_eq!(last_two[0], &SatToken::Enum("double".to_string()));
        assert_eq!(last_two[1], &SatToken::Enum("reversed".to_string()));
    }

    #[test]
    fn sab_preserves_nurbs_integer_fields() {
        let mut doc = SatDocument::new_body();
        let knots = [(0.0, 3), (1.0, 3)];
        let _curve = doc.add_spline_curve(
            true,
            2,
            false,
            &knots,
            &[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]],
            Some(&[1.0, 0.5, 1.0]),
            1.0e-9,
        );
        let surface = doc.add_spline_surface(
            false,
            true,
            1,
            1,
            false,
            false,
            &[(0.0, 2), (1.0, 2)],
            &[(0.0, 2), (1.0, 2)],
            &[
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            Some(&[1.0, 0.5, 0.75, 1.0]),
            1.0e-9,
        );
        let _pcurve = doc.add_pcurve(
            false,
            1,
            false,
            &[(0.0, 2), (1.0, 2)],
            &[[0.0, 0.0], [1.0, 1.0]],
            None,
            1.0e-9,
            surface,
            (0.0, 0.0),
        );

        let roundtrip = SabReader::read(&SabWriter::write(&doc)).unwrap();
        let curve = SatIntCurve::from_record(roundtrip.records_of_type("intcurve-curve")[0])
            .expect("NURBS curve record");
        assert_eq!(curve.bspline().expect("decoded NURBS curve").0, 2);
        let surface = SatSplineSurface::from_record(roundtrip.records_of_type("spline-surface")[0])
            .expect("NURBS surface record");
        let decoded_surface = surface.bspline(&roundtrip).expect("decoded NURBS surface");
        assert_eq!((decoded_surface.degree_u, decoded_surface.degree_v), (1, 1));
        assert_eq!(
            (
                decoded_surface.control_count_u,
                decoded_surface.control_count_v
            ),
            (2, 2)
        );
        assert_eq!(decoded_surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            decoded_surface.control_points,
            vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.5, 0.0, 0.0, 0.5],
                [0.0, 0.75, 0.0, 0.75],
                [1.0, 1.0, 0.0, 1.0]
            ]
        );
        let tokens = &roundtrip.records_of_type("spline-surface")[0].tokens;
        let block = tokens
            .iter()
            .position(|t| t.as_ident() == Some("nurbs"))
            .unwrap();
        assert!(matches!(
            tokens[block],
            SatToken::Sab {
                tag: tags::ENTITY_TYPE,
                ..
            }
        ));
        assert!(matches!(
            tokens[block + 1],
            SatToken::Sab {
                tag: tags::INTEGER,
                ..
            }
        ));
        assert_eq!(tokens[block + 3].as_ident(), Some("both"));
        assert!(matches!(
            tokens[block + 4],
            SatToken::Sab {
                tag: tags::ENUM,
                ..
            }
        ));
        assert!(matches!(
            tokens[block + 10],
            SatToken::Sab {
                tag: tags::DOUBLE,
                ..
            }
        ));
        assert!(matches!(
            tokens[block + 11],
            SatToken::Sab {
                tag: tags::INTEGER,
                ..
            }
        ));
        let pcurve = SatPCurve::from_record(roundtrip.records_of_type("pcurve")[0])
            .expect("NURBS pcurve record");
        assert_eq!(
            pcurve
                .bspline_in(&roundtrip)
                .expect("decoded NURBS pcurve")
                .0,
            1
        );
    }

    #[test]
    fn sat_integer_spline_coordinates_survive_sab() {
        let mut doc = SatDocument::new_body();
        doc.add_spline_curve(
            false,
            2,
            false,
            &[(0.0, 3), (1.0, 3)],
            &[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]],
            None,
            0.0,
        );

        // The SAT parser represents exact integral coordinates as integer
        // tokens; SAB retains their integer tags even though they remain valid
        // numeric spline coordinates.
        let parsed_text = SatDocument::parse(&doc.to_sat_string()).unwrap();
        let roundtrip = SabReader::read(&SabWriter::write(&parsed_text)).unwrap();
        let curve = SatIntCurve::from_record(roundtrip.records_of_type("intcurve-curve")[0])
            .expect("NURBS curve record");
        let (_, _, controls) = curve.bspline().expect("decoded NURBS curve");
        assert_eq!(controls[1], [1.0, 1.0, 0.0, 1.0]);
    }
}
