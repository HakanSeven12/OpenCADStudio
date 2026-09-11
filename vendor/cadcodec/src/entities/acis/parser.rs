//! SAT (Save and Restore) text format parser.
//!
//! Parses ACIS SAT text data into a [`SatDocument`] structure. Supports
//! SAT versions 400–700+ (ACIS 4.0 to 7.0+).

use super::types::*;

/// Parser for ACIS SAT text format.
pub struct SatParser;

impl SatParser {
    /// Parse SAT text into a [`SatDocument`].
    ///
    /// This handles both legacy (pre-7.0) and modern (7.0+) SAT formats.
    pub fn parse(text: &str) -> Result<SatDocument, SatParseError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(SatParseError::EmptyInput);
        }

        let mut lines = SatLines::new(text);

        // Parse header (line 1): version, num_records, num_bodies, flags
        let header_line = lines.next_line().ok_or(SatParseError::EmptyInput)?;
        let header = Self::parse_header_line(header_line)?;

        // The product-id and tolerance header lines only exist from ACIS 2.0
        // on. Pre-2.0 SAT (e.g. AutoCAD R14's ACIS 1.6, version 106) writes
        // the version line then jumps straight to the records — consuming two
        // phantom header lines there ate the `body` and `lump` records, so
        // every pointer resolved two records off and the solid never meshed.
        // A record line always carries `$` pointers; the header lines never
        // do, so peek: `$` in the next line means the extras are absent.
        let has_header_extras = lines.peek_line().map(|l| !l.contains('$')).unwrap_or(false);
        let (product_id, product_version, date, spatial_res, normal_tol, resfit_tol) =
            if has_header_extras {
                let product_line = lines
                    .next_line()
                    .ok_or(SatParseError::InvalidProductInfo("missing".to_string()))?;
                let (pid, pver, date) = Self::parse_product_line(product_line, &header.version)?;
                let tol_line = lines
                    .next_line()
                    .ok_or(SatParseError::InvalidTolerances("missing".to_string()))?;
                let (sr, nt, rt) = Self::parse_tolerance_line(tol_line)?;
                (pid, pver, date, sr, nt, rt)
            } else {
                (
                    String::new(),
                    String::new(),
                    String::new(),
                    1e-6,
                    1e-10,
                    None,
                )
            };

        let sat_header = SatHeader {
            version: header.version,
            num_records: header.num_records,
            num_bodies: header.num_bodies,
            has_history: header.has_history,
            product_id,
            product_version,
            date,
            spatial_resolution: spatial_res,
            normal_tolerance: normal_tol,
            resfit_tolerance: resfit_tol,
        };

        // Parse entity records
        let records = Self::parse_records(&mut lines, &sat_header.version)?;

        let mut doc = SatDocument {
            header: sat_header,
            records,
        };

        // Update record count
        doc.header.num_records = doc.records.len();

        Ok(doc)
    }

    /// Parse the first header line: `<version> <num_records> <num_bodies> <has_history>`
    fn parse_header_line(line: &str) -> Result<HeaderInfo, SatParseError> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            return Err(SatParseError::InvalidHeader(
                "empty header line".to_string(),
            ));
        }

        let version_num: u32 = parts[0]
            .parse()
            .map_err(|_| SatParseError::InvalidHeader(format!("invalid version: {}", parts[0])))?;

        let num_records = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let num_bodies = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let has_history = parts
            .get(3)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
            != 0;

        Ok(HeaderInfo {
            version: SatVersion::from_sat_number(version_num),
            num_records,
            num_bodies,
            has_history,
        })
    }

    /// Parse the product info line.
    ///
    /// Pre-7.0: `<prod_len> <product> <ver_len> <version> <date...>`
    /// 7.0+: `@<len> <product> @<len> <version> @<len> <date>`
    ///
    /// Note: some v700 files still use legacy numeric-prefixed strings in the
    /// product line while using `@`-prefixed strings in entity records. We
    /// auto-detect the format by checking whether the line starts with `@`.
    fn parse_product_line(
        line: &str,
        _version: &SatVersion,
    ) -> Result<(String, String, String), SatParseError> {
        let trimmed = line.trim_start();
        if trimmed.starts_with('@') {
            // ACIS 7.0+ format with @-prefixed counted strings
            Self::parse_product_line_v7(line)
        } else {
            // Legacy numeric-prefixed format (or v700 files that omit '@' in
            // the product line)
            Self::parse_product_line_legacy(line)
        }
    }

    /// Parse v7+ product line with counted strings: `@<len> <text> @<len> <text> @<len> <text>`
    fn parse_product_line_v7(line: &str) -> Result<(String, String, String), SatParseError> {
        let mut pos = 0;
        let bytes = line.as_bytes();

        let product_id = read_counted_string(bytes, &mut pos).unwrap_or_default();
        let product_version = read_counted_string(bytes, &mut pos).unwrap_or_default();
        let date = read_counted_string(bytes, &mut pos).unwrap_or_else(|| {
            // Remaining text is the date
            line[pos..].trim().to_string()
        });

        Ok((product_id, product_version, date))
    }

    /// Parse legacy product line: `<num_len> <product> <num_len> <version> <num_len> <date>`
    fn parse_product_line_legacy(line: &str) -> Result<(String, String, String), SatParseError> {
        let mut pos = 0;
        let bytes = line.as_bytes();

        let product_id = read_legacy_string(bytes, &mut pos).unwrap_or_default();
        let product_version = read_legacy_string(bytes, &mut pos).unwrap_or_default();
        // Remaining text is the date (or another counted string)
        let date =
            read_legacy_string(bytes, &mut pos).unwrap_or_else(|| line[pos..].trim().to_string());

        Ok((product_id, product_version, date))
    }

    /// Parse the tolerance line: `<spatial_resolution> <normal_tolerance> [<resfit_tolerance>]`
    fn parse_tolerance_line(line: &str) -> Result<(f64, f64, Option<f64>), SatParseError> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let spatial = parts.first().and_then(|s| s.parse().ok()).unwrap_or(1e-06);
        let normal = parts
            .get(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(9.9999999999999995e-07);
        let resfit = parts.get(2).and_then(|s| s.parse().ok());
        Ok((spatial, normal, resfit))
    }

    /// Parse entity records from the remaining lines.
    fn parse_records(
        lines: &mut SatLines<'_>,
        version: &SatVersion,
    ) -> Result<Vec<SatRecord>, SatParseError> {
        let mut records = Vec::new();
        let mut auto_index: i32 = 0;

        // Collect remaining text and split by '#' (record terminator)
        let remaining = lines.remaining();
        if remaining.is_empty() {
            return Ok(records);
        }

        // Remove newlines/carriage returns from record data. DXF stores SAT
        // text in gc=1/3 entries (max 255 chars each), joined with newlines by
        // the reader. When a long SAT record exceeds a gc entry boundary, a
        // word can be split across entries (e.g., "reversed" → "rev\nersed").
        // Since records are '#'-terminated, newlines within record text are
        // purely DXF line-boundary artifacts and can be safely removed.
        let remaining = remaining.replace('\n', "").replace('\r', "");

        // Split by '#' to get individual records, then heal splits that landed
        // inside a string.
        //
        // Only 7.0+ delimits strings with `@`; before that they are written as
        // `<len> <chars>`, so the text may legally contain the record
        // terminator. ODA's builder emits exactly that: `dxid-attrib $-1 $-1 $1
        // $0 8 STEP Id# 99 <guid> #`. Cutting at the inner `#` invents a record,
        // which shifts every later record's implied index, so faces end up
        // pointing at whatever record now sits at their number and the body
        // meshes to nothing.
        let mut record_texts: Vec<String> = Vec::new();
        for piece in remaining.split('#') {
            if starts_record(piece) || record_texts.is_empty() {
                record_texts.push(piece.to_string());
            } else {
                let previous = record_texts.last_mut().expect("non-empty");
                previous.push('#');
                previous.push_str(piece);
            }
        }

        for record_text in record_texts {
            let trimmed = record_text.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Check for end-of-data markers
            if trimmed.starts_with("End-of-ACIS-data") || trimmed.starts_with("End-of-ASM-data") {
                break;
            }

            // Also check if end marker is embedded after some whitespace
            if trimmed.contains("End-of-ACIS-data") || trimmed.contains("End-of-ASM-data") {
                // Try to parse the part before the end marker
                let before_end = if let Some(pos) = trimmed.find("End-of-") {
                    trimmed[..pos].trim()
                } else {
                    ""
                };
                if before_end.is_empty() {
                    break;
                }
                // Parse what's before the end marker
                if let Ok(record) = Self::parse_single_record(before_end, version, &mut auto_index)
                {
                    records.push(record);
                }
                break;
            }

            match Self::parse_single_record(trimmed, version, &mut auto_index) {
                Ok(record) => records.push(record),
                Err(_) => {
                    // Skip unparseable records silently (robustness)
                    continue;
                }
            }
        }

        Ok(records)
    }

    /// Parse a single entity record text (everything between `#` terminators).
    fn parse_single_record(
        text: &str,
        version: &SatVersion,
        auto_index: &mut i32,
    ) -> Result<SatRecord, SatParseError> {
        let mut tokenizer = SatTokenizer::new(text, version.has_counted_strings());

        // Determine the record index
        let index = if version.has_explicit_indices() {
            // ACIS 7.0+: records start with -<index>
            match tokenizer.peek_token() {
                Some(tok) if looks_like_negative_index(tok) => {
                    let idx_str = tokenizer.next_raw_token().unwrap();
                    let idx: i32 = idx_str.parse().unwrap_or(*auto_index);
                    let idx = idx.abs();
                    *auto_index = idx + 1;
                    idx
                }
                _ => {
                    let idx = *auto_index;
                    *auto_index += 1;
                    idx
                }
            }
        } else {
            let idx = *auto_index;
            *auto_index += 1;
            idx
        };

        // Entity type
        let entity_type = tokenizer
            .next_raw_token()
            .ok_or(SatParseError::InvalidRecord {
                line: 0,
                message: format!("missing entity type in: {}", text),
            })?;

        // Skip if it's a numeric subtype indicator that's part of the entity type
        // (e.g., "plane-surface" is one entity type)
        let entity_type = entity_type.to_string();

        // Attribute pointer (first token after entity type)
        let attribute = match tokenizer.next_token() {
            Some(SatToken::Pointer(p)) => p,
            Some(SatToken::Integer(v)) => {
                // Sometimes the attribute slot is just an integer
                if v == -1 {
                    SatPointer::NULL
                } else {
                    SatPointer::new(v as i32)
                }
            }
            _ => SatPointer::NULL,
        };

        // Subtype/ID field (integer after attribute, typically -1)
        // Only present in ACIS 7.0+ (SAT version 700+)
        let subtype_id = if version.major >= 7 {
            match tokenizer.peek_token() {
                Some(tok)
                    if !tok.starts_with('$') && !tok.starts_with('@') && !tok.starts_with('#') =>
                {
                    // Check if it looks like a bare integer (digits, possibly negative)
                    if tok.parse::<i64>().is_ok()
                        && !tok.contains('.')
                        && !tok.contains('e')
                        && !tok.contains('E')
                    {
                        // Consume it as the subtype_id
                        match tokenizer.next_token() {
                            Some(SatToken::Integer(v)) => v as i32,
                            _ => -1,
                        }
                    } else {
                        -1
                    }
                }
                _ => -1,
            }
        } else {
            -1
        };

        // Remaining tokens
        let mut tokens = Vec::new();
        while let Some(token) = tokenizer.next_token() {
            if matches!(token, SatToken::Terminator) {
                break;
            }
            tokens.push(token);
        }

        // Normalize v400 (pre-7.0) records to v700 token layout.
        //
        // ACIS 7.0+ added an extra sentinel `$-1` pointer to most entity
        // records (right after the attribute/id fields).  Without this
        // normalization step all typed accessors (SatFace, SatBody, …)
        // would need version-aware token indexing.  Inserting the
        // synthetic sentinel here keeps every downstream consumer simple.
        if version.major < 7 {
            normalize_v400_tokens(&entity_type, &mut tokens);
        }

        Ok(SatRecord {
            index,
            entity_type,
            sub_type: None,
            attribute,
            subtype_id,
            tokens,
            raw_text: Some(text.to_string()),
        })
    }
}

// ============================================================================
// Helper: Header info
// ============================================================================

struct HeaderInfo {
    version: SatVersion,
    num_records: usize,
    num_bodies: usize,
    has_history: bool,
}

// ============================================================================
// Helper: Line iterator
// ============================================================================

/// A line-oriented reader that tracks position in the SAT text.
struct SatLines<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> SatLines<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, pos: 0 }
    }

    /// Peek the next line without consuming it.
    fn peek_line(&self) -> Option<&'a str> {
        if self.pos >= self.text.len() {
            return None;
        }
        let remaining = &self.text[self.pos..];
        let end = remaining.find('\n').unwrap_or(remaining.len());
        Some(remaining[..end].trim_end_matches('\r'))
    }

    /// Read the next line (up to the next newline).
    fn next_line(&mut self) -> Option<&'a str> {
        if self.pos >= self.text.len() {
            return None;
        }

        let remaining = &self.text[self.pos..];
        let end = remaining.find('\n').unwrap_or(remaining.len());
        let line = &remaining[..end];
        self.pos += end + 1; // skip the newline
        Some(line.trim_end_matches('\r'))
    }

    /// Return all remaining text from the current position.
    fn remaining(&self) -> &'a str {
        if self.pos >= self.text.len() {
            ""
        } else {
            &self.text[self.pos..]
        }
    }
}

// ============================================================================
// Helper: SAT Tokenizer
// ============================================================================

/// Tokenizer for SAT record data.
struct SatTokenizer<'a> {
    text: &'a str,
    pos: usize,
    use_counted_strings: bool,
}

impl<'a> SatTokenizer<'a> {
    fn new(text: &'a str, use_counted_strings: bool) -> Self {
        Self {
            text,
            pos: 0,
            use_counted_strings,
        }
    }

    /// Skip whitespace.
    fn skip_whitespace(&mut self) {
        while self.pos < self.text.len() {
            let ch = self.text.as_bytes()[self.pos];
            if ch == b' ' || ch == b'\t' || ch == b'\r' || ch == b'\n' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Peek at the next raw token without consuming it.
    fn peek_token(&self) -> Option<&'a str> {
        let mut pos = self.pos;
        // Skip whitespace
        while pos < self.text.len() {
            let ch = self.text.as_bytes()[pos];
            if ch == b' ' || ch == b'\t' || ch == b'\r' || ch == b'\n' {
                pos += 1;
            } else {
                break;
            }
        }
        if pos >= self.text.len() {
            return None;
        }
        // Read until next whitespace
        let start = pos;
        while pos < self.text.len() {
            let ch = self.text.as_bytes()[pos];
            if ch == b' ' || ch == b'\t' || ch == b'\r' || ch == b'\n' {
                break;
            }
            pos += 1;
        }
        Some(&self.text[start..pos])
    }

    /// Read the next raw token (space-delimited word).
    fn next_raw_token(&mut self) -> Option<&'a str> {
        self.skip_whitespace();
        if self.pos >= self.text.len() {
            return None;
        }
        let start = self.pos;
        while self.pos < self.text.len() {
            let ch = self.text.as_bytes()[self.pos];
            if ch == b' ' || ch == b'\t' || ch == b'\r' || ch == b'\n' {
                break;
            }
            self.pos += 1;
        }
        Some(&self.text[start..self.pos])
    }

    /// Read the next typed token.
    fn next_token(&mut self) -> Option<SatToken> {
        self.skip_whitespace();
        if self.pos >= self.text.len() {
            return None;
        }

        let ch = self.text.as_bytes()[self.pos];

        // Counted string: @<len> <text>
        if ch == b'@' && self.use_counted_strings {
            self.pos += 1; // skip '@'
            let raw = self.next_raw_token()?;
            let len: usize = raw.parse().unwrap_or(0);
            self.skip_whitespace();
            if self.pos + len <= self.text.len() {
                let s = &self.text[self.pos..self.pos + len];
                self.pos += len;
                return Some(SatToken::String(s.to_string()));
            } else {
                // Read remaining as string
                let s = &self.text[self.pos..];
                self.pos = self.text.len();
                return Some(SatToken::String(s.trim().to_string()));
            }
        }

        // Pointer: $<index>
        if ch == b'$' {
            self.pos += 1; // skip '$'
            let raw = self.next_raw_token()?;
            let idx: i32 = raw.parse().unwrap_or(-1);
            return Some(SatToken::Pointer(SatPointer::new(idx)));
        }

        // Terminator
        if ch == b'#' {
            self.pos += 1;
            return Some(SatToken::Terminator);
        }

        // Number or negative index or identifier
        let raw = self.next_raw_token()?;

        // Try integer
        if let Ok(v) = raw.parse::<i64>() {
            return Some(SatToken::Integer(v));
        }

        // Try float
        if let Ok(v) = raw.parse::<f64>() {
            return Some(SatToken::Float(v));
        }

        // Boolean
        if raw.eq_ignore_ascii_case("true") || raw == "T" {
            return Some(SatToken::True);
        }
        if raw.eq_ignore_ascii_case("false") || raw == "F" {
            return Some(SatToken::False);
        }

        // Sense/sidedness enum
        match raw.to_lowercase().as_str() {
            "forward" | "reversed" | "single" | "double" | "in" | "out" | "unknown" => {
                return Some(SatToken::Enum(raw.to_lowercase()));
            }
            _ => {}
        }

        // Identifier
        Some(SatToken::Ident(raw.to_string()))
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Whether a `#`-delimited fragment opens a new entity record.
///
/// Used to tell a real record boundary from a `#` that occurred inside a
/// pre-7.0 counted string. A record opens with its save identifier — always
/// alphabetic, e.g. `body`, `plane-surface`, `rgb_color-st-attrib` — optionally
/// behind a `-<n>` sequence number in 7.0+ files, and the identifier is
/// followed by the entity's attribute slot, written either as a `$` pointer or
/// as a bare integer. A string tail matches none of that.
fn starts_record(fragment: &str) -> bool {
    let head = fragment.trim_start();
    // Nothing to glue onto, and the end marker carries no attribute slot.
    if head.is_empty() || head.starts_with("End-of-") {
        return true;
    }
    let mut rest = head;
    if let Some(after_sign) = head.strip_prefix('-') {
        let tail = after_sign.trim_start_matches(|c: char| c.is_ascii_digit());
        if tail.len() == after_sign.len() || !tail.starts_with(char::is_whitespace) {
            return false;
        }
        rest = tail.trim_start();
    }
    let mut tokens = rest.split_whitespace();
    let Some(identifier) = tokens.next() else {
        return false;
    };
    if !identifier.starts_with(|c: char| c.is_ascii_alphabetic())
        || !identifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return false;
    }
    match tokens.next() {
        Some(slot) => slot.starts_with('$') || slot.parse::<i64>().is_ok(),
        None => false,
    }
}

/// Check if a token looks like a negative index (e.g. "-0", "-1", "-42").
fn looks_like_negative_index(s: &str) -> bool {
    if !s.starts_with('-') {
        return false;
    }
    s[1..].chars().all(|c| c.is_ascii_digit())
}

/// Normalize a pre-7.0 (v400) record's token list to match the 7.0+ layout.
///
/// ACIS 7.0 added a sentinel `$-1` pointer to most entity record types.
/// The position of that sentinel varies by entity type:
///
/// | Entity types                                       | Sentinel position |
/// |----------------------------------------------------|-------------------|
/// | body, subshell, wire, face, loop, coedge, edge,    | 0                 |
/// | vertex,                                            |                   |
/// | point, transform, *-surface, *-curve               |                   |
/// | lump                                               | 1                 |
/// | shell                                              | 2                 |
///
/// By inserting a synthetic `$-1` here the rest of the codebase can use
/// a single set of token indices regardless of the source ACIS version.
pub(crate) fn normalize_v400_tokens(entity_type: &str, tokens: &mut Vec<SatToken>) {
    let base = base_entity_type(entity_type);
    let position = match base {
        "lump" => 1.min(tokens.len()),
        "shell" => 2.min(tokens.len()),
        "body" | "subshell" | "wire" | "face" | "loop" | "vertex" | "coedge" | "edge" | "point"
        | "transform" | "surface" | "curve" | "pcurve" => 0,
        // Unknown entity types (attributes, etc.): leave unchanged.
        _ => return,
    };
    let sentinel = SatToken::Pointer(SatPointer::NULL);
    tokens.insert(position, sentinel);

    // v400 edge is `$sv $ev $coedge $curve sense` — it omits the
    // start_param / end_param doubles that v700 carries between the
    // vertices (`$sv sp $ev ep $coedge $curve sense`). Insert zero
    // placeholders so the fixed-index accessors read derived edge classes too.
    if base == "edge"
        && tokens.len() >= 3
        && matches!(tokens.get(1), Some(SatToken::Pointer(_)))
        && matches!(tokens.get(2), Some(SatToken::Pointer(_)))
    {
        tokens.insert(2, SatToken::Float(0.0));
        if tokens.len() >= 5 {
            tokens.insert(4, SatToken::Float(0.0));
        }
    }
}

/// Read a `@<len> <text>` counted string.
fn read_counted_string(bytes: &[u8], pos: &mut usize) -> Option<String> {
    // Skip whitespace
    while *pos < bytes.len() && (bytes[*pos] == b' ' || bytes[*pos] == b'\t') {
        *pos += 1;
    }

    if *pos >= bytes.len() || bytes[*pos] != b'@' {
        return None;
    }
    *pos += 1; // skip '@'

    // Read length
    let start = *pos;
    while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
        *pos += 1;
    }
    let len_str = std::str::from_utf8(&bytes[start..*pos]).ok()?;
    let len: usize = len_str.parse().ok()?;

    // Skip single space
    if *pos < bytes.len() && bytes[*pos] == b' ' {
        *pos += 1;
    }

    // Read exactly `len` bytes
    if *pos + len <= bytes.len() {
        let s = std::str::from_utf8(&bytes[*pos..*pos + len]).ok()?;
        *pos += len;
        Some(s.to_string())
    } else {
        let s = std::str::from_utf8(&bytes[*pos..]).ok()?;
        *pos = bytes.len();
        Some(s.trim().to_string())
    }
}

/// Read a legacy `<num> <text>` string where `<num>` is the character count.
fn read_legacy_string(bytes: &[u8], pos: &mut usize) -> Option<String> {
    // Skip whitespace
    while *pos < bytes.len() && (bytes[*pos] == b' ' || bytes[*pos] == b'\t') {
        *pos += 1;
    }

    if *pos >= bytes.len() {
        return None;
    }

    // Read length number
    let start = *pos;
    while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
        *pos += 1;
    }

    if start == *pos {
        // No number found — not a counted string
        return None;
    }

    let len_str = std::str::from_utf8(&bytes[start..*pos]).ok()?;
    let len: usize = len_str.parse().ok()?;

    // Skip single space
    if *pos < bytes.len() && bytes[*pos] == b' ' {
        *pos += 1;
    }

    // Read exactly `len` bytes
    if *pos + len <= bytes.len() {
        let s = std::str::from_utf8(&bytes[*pos..*pos + len]).ok()?;
        *pos += len;
        Some(s.to_string())
    } else {
        let s = std::str::from_utf8(&bytes[*pos..]).ok()?;
        *pos = bytes.len();
        Some(s.trim().to_string())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_header_line() {
        let info = SatParser::parse_header_line("700 0 1 0").unwrap();
        assert_eq!(info.version, SatVersion::new(7, 0, 0));
        assert_eq!(info.num_records, 0);
        assert_eq!(info.num_bodies, 1);
        assert!(!info.has_history);
    }

    #[test]
    fn test_parse_header_line_v400() {
        let info = SatParser::parse_header_line("400 0 1 0").unwrap();
        assert_eq!(info.version, SatVersion::new(4, 0, 0));
    }

    #[test]
    fn test_parse_tolerance_line() {
        let (spatial, normal, resfit) =
            SatParser::parse_tolerance_line("1e-06 9.9999999999999995e-07").unwrap();
        assert!((spatial - 1e-06).abs() < 1e-20);
        assert!((normal - 9.9999999999999995e-07).abs() < 1e-20);
        assert!(resfit.is_none());

        // With resfit value (v7+ format)
        let (spatial, normal, resfit) =
            SatParser::parse_tolerance_line("1e-06 9.9999999999999995e-07 1e-10").unwrap();
        assert!((spatial - 1e-06).abs() < 1e-20);
        assert!((normal - 9.9999999999999995e-07).abs() < 1e-20);
        assert!((resfit.unwrap() - 1e-10).abs() < 1e-24);
    }

    #[test]
    fn test_parse_simple_v700_document() {
        let sat = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            1e-06 9.9999999999999995e-07\n\
            -0 asmheader $-1 -1 @12 700 7 0 0 @5 ACIS @3 7.0 @24 Thu Jan 01 00:00:00 2023 #\n\
            -1 body $-1 $-1 $-1 $-1 #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat).unwrap();
        assert_eq!(doc.header.version, SatVersion::V7_0);
        assert_eq!(doc.records.len(), 2);
        assert_eq!(doc.records[0].entity_type, "asmheader");
        assert_eq!(doc.records[0].index, 0);
        assert_eq!(doc.records[1].entity_type, "body");
        assert_eq!(doc.records[1].index, 1);
    }

    #[test]
    fn test_parse_simple_v400_document() {
        let sat = "400 0 1 0\n\
            19 Spatial ACIS Modeler 7 ACIS 4.0 24 Thu Jan 01 00:00:00 2023\n\
            1e-06 9.9999999999999995e-07\n\
            body $-1 $1 $-1 $-1 #\n\
            lump $-1 $-1 $2 $0 #\n\
            shell $-1 $-1 $-1 $3 $-1 $1 #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat).unwrap();
        assert_eq!(doc.header.version, SatVersion::new(4, 0, 0));
        assert_eq!(doc.records.len(), 3);
        assert_eq!(doc.records[0].entity_type, "body");
        assert_eq!(doc.records[0].index, 0);
        assert_eq!(doc.records[1].entity_type, "lump");
        assert_eq!(doc.records[1].index, 1);
        assert_eq!(doc.records[2].entity_type, "shell");
        assert_eq!(doc.records[2].index, 2);
    }

    #[test]
    fn test_parse_pointers() {
        let sat = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            1e-06 9.9999999999999995e-07\n\
            -0 body $-1 $1 $-1 $-1 #\n\
            -1 lump $-1 $-1 $2 $0 #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat).unwrap();
        let body = &doc.records[0];
        assert_eq!(body.attribute, SatPointer::NULL);
        assert_eq!(body.token_pointer(0), Some(SatPointer::new(1)));
        assert_eq!(body.token_pointer(1), Some(SatPointer::NULL));
    }

    #[test]
    fn test_body_accessor() {
        let sat = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            1e-06 9.9999999999999995e-07\n\
            body $-1 -1 $-1 $1 $-1 $2 #\n\
            lump $-1 -1 $-1 $-1 $-1 $0 #\n\
            transform $-1 -1 $-1 1 0 0 0 1 0 0 0 1 0 0 0 1 #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat).unwrap();
        let bodies = doc.bodies();
        assert_eq!(bodies.len(), 1);
        let body = &bodies[0];
        assert_eq!(body.lump(), SatPointer::new(1));
        assert_eq!(body.wire_body(), SatPointer::NULL);
        assert_eq!(body.transform(), SatPointer::new(2));
    }

    #[test]
    fn test_plane_surface_accessor() {
        let sat = "700 0 0 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            1e-06 9.9999999999999995e-07\n\
            -0 plane-surface $-1 -1 $-1 1.0 2.0 3.0 0 0 1 1 0 0 forward_v I I I I #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat).unwrap();
        let records = doc.records_of_type("plane-surface");
        assert_eq!(records.len(), 1);
        let plane = SatPlaneSurface::from_record(records[0]).unwrap();
        assert_eq!(plane.root_point(), (1.0, 2.0, 3.0));
        assert_eq!(plane.normal(), (0.0, 0.0, 1.0));
        assert_eq!(plane.u_direction(), (1.0, 0.0, 0.0));
    }

    #[test]
    fn test_validate_document() {
        let sat = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            1e-06 9.9999999999999995e-07\n\
            -0 body $-1 $99 $-1 $-1 #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat).unwrap();
        let errors = doc.validate();
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_version_properties() {
        let v7 = SatVersion::V7_0;
        assert!(v7.has_explicit_indices());
        assert!(v7.has_counted_strings());
        assert!(v7.has_asm_header());
        assert_eq!(v7.sat_version_number(), 700);

        let v4 = SatVersion::V4_0;
        assert!(!v4.has_explicit_indices());
        assert!(!v4.has_counted_strings());
        assert!(!v4.has_asm_header());
        assert_eq!(v4.sat_version_number(), 400);
    }

    #[test]
    fn test_counted_string_reading() {
        let input = b"@5 hello @3 bye";
        let mut pos = 0;
        let s1 = read_counted_string(input, &mut pos).unwrap();
        assert_eq!(s1, "hello");
        let s2 = read_counted_string(input, &mut pos).unwrap();
        assert_eq!(s2, "bye");
    }

    #[test]
    fn test_legacy_string_reading() {
        let input = b"5 hello 3 bye";
        let mut pos = 0;
        let s1 = read_legacy_string(input, &mut pos).unwrap();
        assert_eq!(s1, "hello");
        let s2 = read_legacy_string(input, &mut pos).unwrap();
        assert_eq!(s2, "bye");
    }

    #[test]
    fn test_parse_box_sat() {
        // A simplified SAT representation of a box (v700 format with subtype_id)
        let sat = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            1e-06 9.9999999999999995e-07\n\
            asmheader $-1 -1 @12 700 7 0 0 @5 ACIS @3 7.0 @24 Thu Jan 01 00:00:00 2023 #\n\
            body $-1 -1 $-1 $2 $-1 $-1 #\n\
            lump $-1 -1 $-1 $-1 $3 $1 #\n\
            shell $-1 -1 $-1 $-1 $-1 $4 $-1 $2 #\n\
            face $-1 -1 $-1 $5 $10 $3 $-1 $20 forward single #\n\
            face $-1 -1 $-1 $6 $11 $3 $-1 $21 forward single #\n\
            face $-1 -1 $-1 $7 $12 $3 $-1 $22 forward single #\n\
            face $-1 -1 $-1 $8 $13 $3 $-1 $23 forward single #\n\
            face $-1 -1 $-1 $9 $14 $3 $-1 $24 forward single #\n\
            face $-1 -1 $-1 $-1 $15 $3 $-1 $25 forward single #\n\
            loop $-1 -1 $-1 $-1 $30 $4 #\n\
            loop $-1 -1 $-1 $-1 $31 $5 #\n\
            loop $-1 -1 $-1 $-1 $32 $6 #\n\
            loop $-1 -1 $-1 $-1 $33 $7 #\n\
            loop $-1 -1 $-1 $-1 $34 $8 #\n\
            loop $-1 -1 $-1 $-1 $35 $9 #\n\
            plane-surface $-1 -1 $-1 0 0 5 0 0 1 1 0 0 forward_v I I I I #\n\
            plane-surface $-1 -1 $-1 0 0 -5 0 0 -1 1 0 0 forward_v I I I I #\n\
            plane-surface $-1 -1 $-1 5 0 0 1 0 0 0 1 0 forward_v I I I I #\n\
            plane-surface $-1 -1 $-1 -5 0 0 -1 0 0 0 1 0 forward_v I I I I #\n\
            plane-surface $-1 -1 $-1 0 5 0 0 1 0 0 0 1 forward_v I I I I #\n\
            plane-surface $-1 -1 $-1 0 -5 0 0 -1 0 0 0 1 forward_v I I I I #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat).unwrap();
        assert_eq!(doc.header.version, SatVersion::V7_0);

        // Should have: 1 asmheader + 1 body + 1 lump + 1 shell + 6 faces + 6 loops + 6 surfaces = 22
        assert_eq!(doc.records.len(), 22);

        // Check body
        let bodies = doc.bodies();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].lump(), SatPointer::new(2));

        // Check faces
        let faces = doc.faces();
        assert_eq!(faces.len(), 6);

        // Check surfaces
        let planes = doc.records_of_type("plane-surface");
        assert_eq!(planes.len(), 6);

        // Check top surface
        let top = SatPlaneSurface::from_record(planes[0]).unwrap();
        assert_eq!(top.root_point(), (0.0, 0.0, 5.0));
        assert_eq!(top.normal(), (0.0, 0.0, 1.0));
    }

    #[test]
    fn test_entity_classification() {
        assert_eq!(classify_entity_type("body"), SatEntityCategory::Topology);
        assert_eq!(classify_entity_type("face"), SatEntityCategory::Topology);
        assert_eq!(
            classify_entity_type("plane-surface"),
            SatEntityCategory::Geometry
        );
        assert_eq!(
            classify_entity_type("straight-curve"),
            SatEntityCategory::Geometry
        );
        assert_eq!(
            classify_entity_type("transform"),
            SatEntityCategory::Transform
        );
        assert_eq!(classify_entity_type("asmheader"), SatEntityCategory::Header);
    }

    #[test]
    fn test_v700_legacy_product_line() {
        // v700 file that uses legacy numeric counted strings in the product
        // line (not @-prefixed). This is common in practice.
        let sat = "700 0 2 0\n\
            19 TransMagic R7 sp0.0 14 ACIS 16.0.7 NT 24 Tue Mar 20 12:06:10 2007\n\
            25.399999999999999 9.9999999999999995e-007 1e-010\n\
            body $2 -1 $-1 $3 $-1 $-1 #\n\
            lump $4 -1 $-1 $-1 $5 $0 #\n\
            End-of-ACIS-data\n";

        let doc = SatDocument::parse(sat).unwrap();
        assert_eq!(doc.header.version, SatVersion::V7_0);
        assert_eq!(doc.header.product_id, "TransMagic R7 sp0.0");
        assert_eq!(doc.header.product_version, "ACIS 16.0.7 NT");
        assert_eq!(doc.header.date, "Tue Mar 20 12:06:10 2007");
        assert_eq!(doc.records.len(), 2);
        assert_eq!(doc.records[0].entity_type, "body");
        assert_eq!(doc.records[1].entity_type, "lump");
    }

    #[test]
    fn test_parse_sample_sat_1() {
        let path = "examples/sat v7 samples/sample_sat_1.sat";
        let Ok(sat) = std::fs::read_to_string(path) else {
            return;
        };
        let doc = SatDocument::parse(&sat).unwrap();

        assert_eq!(doc.header.version, SatVersion::V7_0);
        assert_eq!(doc.header.product_id, "TransMagic R7 sp0.0");
        assert_eq!(doc.header.product_version, "ACIS 16.0.7 NT");
        assert_eq!(doc.header.date, "Tue Mar 20 12:06:10 2007");

        // Header says 2 bodies
        let bodies = doc.bodies();
        assert_eq!(bodies.len(), 2);

        // Should have shells, faces, loops, edges, etc.
        let faces = doc.faces();
        assert!(!faces.is_empty(), "expected face records");

        let shells = doc.records_of_type("shell");
        assert!(!shells.is_empty(), "expected shell records");

        let lumps = doc.records_of_type("lump");
        assert_eq!(lumps.len(), 2);

        // Should have many records (this is a complex model)
        assert!(
            doc.records.len() > 100,
            "expected >100 records, got {}",
            doc.records.len()
        );

        println!(
            "sample_sat_1: {} records, {} bodies, {} faces, {} shells",
            doc.records.len(),
            bodies.len(),
            faces.len(),
            shells.len()
        );
    }

    #[test]
    fn test_parse_sample_sat_2() {
        let path = "examples/sat v7 samples/sample_sat_2.sat";
        let Ok(sat) = std::fs::read_to_string(path) else {
            return;
        };
        let doc = SatDocument::parse(&sat).unwrap();

        assert_eq!(doc.header.version, SatVersion::V7_0);
        assert_eq!(doc.header.product_id, "TransMagic R7 sp0.0");
        assert_eq!(doc.header.product_version, "ACIS 16.0.7 NT");
        assert_eq!(doc.header.date, "Tue Mar 20 12:06:38 2007");

        // Header says 9 bodies
        let bodies = doc.bodies();
        assert_eq!(bodies.len(), 9);

        let lumps = doc.records_of_type("lump");
        assert!(lumps.len() >= 9, "expected at least 9 lumps");

        let shells = doc.records_of_type("shell");
        assert!(!shells.is_empty(), "expected shell records");

        let faces = doc.faces();
        assert!(!faces.is_empty(), "expected face records");

        // Has cone surfaces and plane surfaces
        let cones = doc.records_of_type("cone-surface");
        assert!(!cones.is_empty(), "expected cone-surface records");

        let planes = doc.records_of_type("plane-surface");
        assert!(!planes.is_empty(), "expected plane-surface records");

        // Has spline surfaces
        let splines = doc.records_of_type("spline-surface");
        assert!(!splines.is_empty(), "expected spline-surface records");

        // Should have many records (this is a complex model with 9 bodies)
        assert!(
            doc.records.len() > 500,
            "expected >500 records, got {}",
            doc.records.len()
        );

        println!(
            "sample_sat_2: {} records, {} bodies, {} faces, {} cones, {} planes, {} splines",
            doc.records.len(),
            bodies.len(),
            faces.len(),
            cones.len(),
            planes.len(),
            splines.len()
        );
    }
}
