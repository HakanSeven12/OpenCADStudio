//! High-performance text search and replacement operations for OpenCADStudio.
//!
//! Provides headless, control-bridge, and MCP-accessible text querying and batch
//! replacement across Text, MText, Dimension, AttributeDefinition, and Insert entities.
//! Safely preserves MText inline formatting codes (fonts, colors, heights, braces)
//! and handles DXF Unicode escapes (`\U+XXXX`) and special character codes (`%%d`, `%%p`, etc.).

use super::{failure, OpenCADStudio};
use crate::app::Message;
use codec::{CadDocument, EntityType, Handle};
use iced::Task;
use serde_json::{json, Value};

/// Decodes DXF special character codes (`%%d`, `%%p`, `%%c`, `%%nnn`) and
/// DXF Unicode escape sequences (`\U+XXXX` / `\u+XXXX`) into UTF-8 characters.
pub fn decode_dxf_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' && chars.peek() == Some(&'%') {
            chars.next(); // consume second '%'
            match chars.peek().map(|ch| ch.to_ascii_lowercase()) {
                Some('d') => {
                    chars.next();
                    out.push('°');
                }
                Some('p') => {
                    chars.next();
                    out.push('±');
                }
                Some('c') => {
                    chars.next();
                    out.push('∅');
                }
                Some('u') | Some('o') => {
                    chars.next(); // toggle codes — strip silently
                }
                Some('%') => {
                    chars.next();
                    if chars.peek() == Some(&'%') {
                        chars.next();
                    }
                    out.push('%');
                }
                Some(d) if d.is_ascii_digit() => {
                    let mut digits = String::with_capacity(3);
                    for _ in 0..3 {
                        match chars.peek() {
                            Some(&ch) if ch.is_ascii_digit() => {
                                digits.push(chars.next().unwrap());
                            }
                            _ => break,
                        }
                    }
                    if digits.len() == 3 {
                        if let Ok(n) = digits.parse::<u32>() {
                            let ch_opt = match n {
                                128 => Some('€'),
                                130 => Some('‚'),
                                131 => Some('ƒ'),
                                132 => Some('„'),
                                133 => Some('…'),
                                134 => Some('†'),
                                135 => Some('‡'),
                                136 => Some('ˆ'),
                                137 => Some('‰'),
                                138 => Some('Š'),
                                139 => Some('‹'),
                                140 => Some('Œ'),
                                142 => Some('Ž'),
                                145 => Some('‘'),
                                146 => Some('’'),
                                147 => Some('“'),
                                148 => Some('”'),
                                149 => Some('•'),
                                150 => Some('–'),
                                151 => Some('—'),
                                152 => Some('˜'),
                                153 => Some('™'),
                                154 => Some('š'),
                                155 => Some('›'),
                                156 => Some('œ'),
                                158 => Some('ž'),
                                159 => Some('Ÿ'),
                                160..=255 => char::from_u32(n),
                                _ => char::from_u32(n),
                            };
                            if let Some(ch) = ch_opt {
                                out.push(ch);
                                continue;
                            }
                        }
                    }
                    out.push('%');
                    out.push('%');
                    out.push_str(&digits);
                }
                _ => {
                    out.push('%');
                    out.push('%');
                }
            }
            continue;
        }

        // DXF Unicode escape: \U+XXXX or \u+XXXX
        if c == '\\' && (chars.peek() == Some(&'U') || chars.peek() == Some(&'u')) {
            let mut clone = chars.clone();
            clone.next(); // consume U/u
            if clone.peek() == Some(&'+') {
                clone.next(); // consume '+'
                let mut hex = String::with_capacity(4);
                for _ in 0..4 {
                    match clone.peek() {
                        Some(&ch) if ch.is_ascii_hexdigit() => {
                            hex.push(clone.next().unwrap());
                        }
                        _ => break,
                    }
                }
                if hex.len() == 4 {
                    if let Ok(codepoint) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(codepoint) {
                            out.push(ch);
                            chars = clone; // advance chars
                            continue;
                        }
                    }
                }
            }
        }

        out.push(c);
    }

    out
}

/// Strips common Latin diacritics/accents into their plain ASCII equivalents (e.g. 'é' -> 'e', 'À' -> 'A').
pub fn remove_diacritics(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'È' | 'É' | 'Ê' | 'Ë' => 'E',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'O',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
            'ý' | 'ÿ' => 'y',
            'Ý' => 'Y',
            'ç' => 'c',
            'Ç' => 'C',
            'ñ' => 'n',
            'Ñ' => 'N',
            other => other,
        })
        .collect()
}

/// Converts LF / CRLF newlines into DXF MText paragraph breaks (`\P`).
pub fn to_raw_mtext_replacement(s: &str) -> String {
    s.replace("\r\n", "\\P").replace('\n', "\\P")
}

/// Converts raw DXF MText paragraph breaks (`\P` / `\p`) into plain text newlines (`\n`).
pub fn to_plain_text_search(s: &str) -> String {
    s.replace("\\P", "\n").replace("\\p", "\n")
}

/// Converts newlines (`\r\n` / `\n`) into raw DXF MText paragraph breaks (`\P`).
pub fn to_raw_mtext_search(s: &str) -> String {
    s.replace("\r\n", "\\P").replace('\n', "\\P")
}

/// Pattern matcher supporting case-sensitive/case-insensitive, whole-word,
/// and accent/diacritic-insensitive matching across Unicode and ASCII strings.
#[derive(Debug, Clone)]
pub struct TextMatcher {
    pub search: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub ignore_accents: bool,
}

impl TextMatcher {
    pub fn new(
        search: String,
        match_case: bool,
        whole_word: bool,
        ignore_accents: bool,
    ) -> Self {
        Self {
            search,
            match_case,
            whole_word,
            ignore_accents,
        }
    }

    pub fn with_search(&self, search: String) -> Self {
        Self {
            search,
            match_case: self.match_case,
            whole_word: self.whole_word,
            ignore_accents: self.ignore_accents,
        }
    }

    pub fn matches(&self, hay: &str) -> bool {
        !self.find_all_in(hay).is_empty()
    }

    /// Matches against both plain display text and raw CAD string, normalizing
    /// newlines and MText paragraph break escapes (`\n` <-> `\P`).
    pub fn matches_text_or_raw(&self, plain_text: &str, raw_val: &str) -> bool {
        if self.matches(plain_text) || self.matches(raw_val) {
            return true;
        }
        if self.search.contains('\n') || self.search.contains("\r\n") {
            let p_matcher = self.with_search(to_raw_mtext_search(&self.search));
            if p_matcher.matches(raw_val) {
                return true;
            }
        }
        if self.search.contains("\\P") || self.search.contains("\\p") {
            let lf_matcher = self.with_search(to_plain_text_search(&self.search));
            if lf_matcher.matches(plain_text) {
                return true;
            }
        }
        false
    }

    /// Finds all non-overlapping match slices `(start_byte, end_byte)` in `hay`.
    pub fn find_all_in(&self, hay: &str) -> Vec<(usize, usize)> {
        if self.search.is_empty() || hay.is_empty() {
            return Vec::new();
        }

        // Fast path: exact case-sensitive, accent-sensitive substring search
        if self.match_case && !self.ignore_accents && !self.whole_word {
            let mut matches = Vec::new();
            let mut cursor = 0;
            while cursor < hay.len() {
                if let Some(idx) = hay[cursor..].find(&self.search) {
                    let start = cursor + idx;
                    let end = start + self.search.len();
                    matches.push((start, end));
                    cursor = end;
                } else {
                    break;
                }
            }
            return matches;
        }

        // Character-indexed search for case-insensitive, whole-word, and/or accent-insensitive matching
        let hay_chars: Vec<char> = hay.chars().collect();
        let mut byte_offsets = Vec::with_capacity(hay_chars.len() + 1);
        let mut cur_byte = 0;
        for &c in &hay_chars {
            byte_offsets.push(cur_byte);
            cur_byte += c.len_utf8();
        }
        byte_offsets.push(cur_byte);

        let needle_norm: Vec<char> = {
            let s = if self.ignore_accents {
                remove_diacritics(&self.search)
            } else {
                self.search.clone()
            };
            if self.match_case {
                s.chars().collect()
            } else {
                s.to_lowercase().chars().collect()
            }
        };

        let hay_norm: Vec<char> = {
            let s = if self.ignore_accents {
                remove_diacritics(hay)
            } else {
                hay.to_string()
            };
            if self.match_case {
                s.chars().collect()
            } else {
                s.to_lowercase().chars().collect()
            }
        };

        let mut matches = Vec::new();
        let n_len = needle_norm.len();
        if n_len > 0 && n_len <= hay_norm.len() {
            let mut i = 0;
            while i + n_len <= hay_norm.len() {
                if hay_norm[i..i + n_len] == needle_norm[..] {
                    let start_byte = byte_offsets[i];
                    let end_byte = byte_offsets[i + n_len];

                    let is_word_boundary = if self.whole_word {
                        let before_ok = if i == 0 {
                            true
                        } else {
                            !is_word_char(hay_chars[i - 1])
                        };
                        let after_ok = if i + n_len >= hay_chars.len() {
                            true
                        } else {
                            !is_word_char(hay_chars[i + n_len])
                        };
                        before_ok && after_ok
                    } else {
                        true
                    };

                    if is_word_boundary {
                        matches.push((start_byte, end_byte));
                        i += n_len;
                        continue;
                    }
                }
                i += 1;
            }
        }

        matches
    }

    /// Replaces occurrences in `hay` with `replacement`.
    pub fn replace_all_in(
        &self,
        hay: &str,
        replacement: &str,
        replace_all: bool,
    ) -> (String, usize) {
        let matches = self.find_all_in(hay);
        if matches.is_empty() {
            return (hay.to_string(), 0);
        }

        let mut result = String::with_capacity(hay.len());
        let mut cursor = 0;
        let mut count = 0;

        for (start, end) in matches {
            result.push_str(&hay[cursor..start]);
            result.push_str(replacement);
            cursor = end;
            count += 1;
            if !replace_all {
                break;
            }
        }
        result.push_str(&hay[cursor..]);
        (result, count)
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn has_trailing_paragraph_break(raw: &str) -> bool {
    let s = raw.trim_end();
    let s = s.strip_suffix('}').unwrap_or(s);
    match s.strip_suffix('P') {
        Some(rest) => rest.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1,
        None => false,
    }
}

/// Safely replaces text within an MText string by modifying text runs inside
/// the parsed MText structure, preserving all font tags, colors, and line breaks.
/// Supports both single-span replacements and multiline replacements across
/// paragraph breaks (`\P` / `\n`).
pub fn replace_mtext_safe(
    raw: &str,
    matcher: &TextMatcher,
    replacement: &str,
    replace_all: bool,
) -> (String, usize) {
    if raw.is_empty() {
        return (raw.to_string(), 0);
    }

    // Convert replacement newlines to \P for MText compatibility
    let raw_replacement = to_raw_mtext_replacement(replacement);

    let mut doc = codec::entities::mtext_format::parse_mtext(raw, true);
    let mut total_replaced = 0;

    // First attempt: replace within individual parsed spans (preserves fine-grained formatting)
    for para in &mut doc.paragraphs {
        for span in &mut para.spans {
            if span.stacking.is_none() {
                let (new_text, count) =
                    matcher.replace_all_in(&span.text, &raw_replacement, replace_all);
                if count > 0 {
                    span.text = new_text;
                    total_replaced += count;
                    if !replace_all {
                        break;
                    }
                }
            }
        }
        if !replace_all && total_replaced > 0 {
            break;
        }
    }

    if total_replaced > 0 {
        if has_trailing_paragraph_break(raw) {
            doc.paragraphs
                .push(codec::entities::mtext_format::MTextParagraph::new());
        }
        (doc.to_mtext_string(), total_replaced)
    } else {
        // Second attempt: raw string replacement
        // Handles multiline text where the search pattern spans across paragraph breaks (\P / \n)
        replace_mtext_raw_flexible(raw, matcher, &raw_replacement, replace_all)
    }
}

/// Helper for flexible raw MText replacement handling newlines and paragraph breaks.
fn replace_mtext_raw_flexible(
    raw: &str,
    matcher: &TextMatcher,
    raw_replacement: &str,
    replace_all: bool,
) -> (String, usize) {
    // 1. Try direct raw replacement with current matcher
    let (direct_res, direct_count) = matcher.replace_all_in(raw, raw_replacement, replace_all);
    if direct_count > 0 {
        return (direct_res, direct_count);
    }

    // 2. If search pattern contains newlines (\r\n or \n), convert them to \P and try
    if matcher.search.contains('\n') || matcher.search.contains("\r\n") {
        let p_search = to_raw_mtext_search(&matcher.search);
        let p_matcher = matcher.with_search(p_search);
        let (p_res, p_count) = p_matcher.replace_all_in(raw, raw_replacement, replace_all);
        if p_count > 0 {
            return (p_res, p_count);
        }

        // 3. Multiline paragraph-boundary matching with flexible whitespace:
        // Handles cases where raw has spaces around \P, or paragraph formatting like \pxqc;
        let lines: Vec<&str> = matcher
            .search
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect();

        if lines.len() >= 2 {
            let (flex_res, flex_count) =
                replace_raw_lines(raw, &lines, matcher, raw_replacement, replace_all);
            if flex_count > 0 {
                return (flex_res, flex_count);
            }
        }
    } else if matcher.search.contains("\\P") || matcher.search.contains("\\p") {
        // Search contained literal \P; try with \n in case raw has raw LF
        let lf_search = to_plain_text_search(&matcher.search);
        let lf_matcher = matcher.with_search(lf_search);
        let (lf_res, lf_count) = lf_matcher.replace_all_in(raw, raw_replacement, replace_all);
        if lf_count > 0 {
            return (lf_res, lf_count);
        }
    }

    (raw.to_string(), 0)
}

/// Helper to match line sequences across raw MText paragraph breaks (\P / \p / tags / spaces).
fn replace_raw_lines(
    raw: &str,
    lines: &[&str],
    matcher: &TextMatcher,
    raw_replacement: &str,
    replace_all: bool,
) -> (String, usize) {
    if lines.len() < 2 || raw.is_empty() {
        return (raw.to_string(), 0);
    }

    let line0 = lines[0].trim_end();
    let line_last = lines[lines.len() - 1].trim_start();
    let middle_lines: Vec<&str> = lines[1..lines.len() - 1].iter().map(|l| l.trim()).collect();

    let m0 = matcher.with_search(line0.to_string());
    let m_last = matcher.with_search(line_last.to_string());
    let m_mids: Vec<TextMatcher> = middle_lines
        .iter()
        .map(|l| matcher.with_search(l.to_string()))
        .collect();

    let matches0 = m0.find_all_in(raw);
    if matches0.is_empty() {
        return (raw.to_string(), 0);
    }

    let mut result = String::with_capacity(raw.len());
    let mut cursor = 0;
    let mut count = 0;

    for (start0, end0) in matches0 {
        if start0 < cursor {
            continue; // already replaced
        }

        // Try to verify subsequent lines separated by MText paragraph breaks
        let mut check_pos = end0;
        let mut matched_all = true;

        for m_mid in &m_mids {
            if let Some(next_pos) = skip_mtext_paragraph_break(raw, check_pos) {
                let rest = &raw[next_pos..];
                let mid_matches = m_mid.find_all_in(rest);
                if let Some(&(m_s, m_e)) = mid_matches.first() {
                    if m_s == 0 {
                        check_pos = next_pos + m_e;
                        continue;
                    }
                }
            }
            matched_all = false;
            break;
        }

        if !matched_all {
            continue;
        }

        if let Some(next_pos) = skip_mtext_paragraph_break(raw, check_pos) {
            let rest = &raw[next_pos..];
            let last_matches = m_last.find_all_in(rest);
            if let Some(&(l_s, l_e)) = last_matches.first() {
                if l_s == 0 {
                    let end_pos = next_pos + l_e;
                    result.push_str(&raw[cursor..start0]);
                    result.push_str(raw_replacement);
                    cursor = end_pos;
                    count += 1;
                    if !replace_all {
                        break;
                    }
                    continue;
                }
            }
        }
    }

    if count > 0 {
        result.push_str(&raw[cursor..]);
        (result, count)
    } else {
        (raw.to_string(), 0)
    }
}

/// Skips over an MText paragraph break: optional whitespace, `\P` or `\p`, optional format tags like `\pxqc;`, optional whitespace.
fn skip_mtext_paragraph_break(raw: &str, from: usize) -> Option<usize> {
    if from >= raw.len() {
        return None;
    }
    let bytes = raw.as_bytes();
    let mut i = from;

    // Skip whitespace before \P
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }

    // Must have \P or \p
    if i + 1 < bytes.len() && bytes[i] == b'\\' && (bytes[i + 1] == b'P' || bytes[i + 1] == b'p') {
        i += 2;
    } else {
        return None;
    }

    // Skip optional paragraph format tags like \pxqc; or \pi...;
    if i < bytes.len() && bytes[i] == b'\\' {
        let mut tag_end = i + 1;
        while tag_end < bytes.len()
            && bytes[tag_end] != b';'
            && bytes[tag_end] != b'\\'
            && bytes[tag_end] != b'{'
            && bytes[tag_end] != b'}'
        {
            tag_end += 1;
        }
        if tag_end < bytes.len() && bytes[tag_end] == b';' {
            i = tag_end + 1;
        }
    }

    // Skip whitespace after \P
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }

    Some(i)
}

/// Extracts the plain display text and position for an entity.
pub fn entity_text_info(
    entity: &EntityType,
    document: &CadDocument,
) -> Option<(String, String, [f64; 3])> {
    // Returns (raw_value, plain_text, [x, y, z])
    match entity {
        EntityType::Text(t) => {
            let raw = t.value.clone();
            let plain = decode_dxf_escapes(&raw);
            let pos = [t.insertion_point.x, t.insertion_point.y, t.insertion_point.z];
            Some((raw, plain, pos))
        }
        EntityType::MText(t) => {
            let raw = t.value.clone();
            let plain_raw = codec::entities::mtext_format::parse_mtext(&raw, true).to_plain_text();
            let plain = decode_dxf_escapes(&plain_raw);
            let pos = [t.insertion_point.x, t.insertion_point.y, t.insertion_point.z];
            Some((raw, plain, pos))
        }
        EntityType::AttributeDefinition(ad) => {
            let raw = ad.default_value.clone();
            let plain = decode_dxf_escapes(&raw);
            let pos = [ad.insertion_point.x, ad.insertion_point.y, ad.insertion_point.z];
            Some((raw, plain, pos))
        }
        EntityType::AttributeEntity(ae) => {
            let raw = ae.get_value().to_string();
            let plain = decode_dxf_escapes(&raw);
            let pos = [ae.insertion_point.x, ae.insertion_point.y, ae.insertion_point.z];
            Some((raw, plain, pos))
        }
        EntityType::Dimension(dim) => {
            let text = dim.base().text.clone();
            if text.is_empty() {
                None
            } else {
                let plain = decode_dxf_escapes(&text);
                let p = dim.base().text_middle_point;
                Some((text, plain, [p.x, p.y, p.z]))
            }
        }
        _ => {
            let _ = document;
            None
        }
    }
}

fn parse_bounds_filter(req: &Value) -> Result<Option<[f64; 4]>, Value> {
    let Some(bounds_val) = req.get("bounds") else {
        return Ok(None);
    };
    let bounds = bounds_val.as_array().and_then(|values| {
        if values.len() != 4 {
            return None;
        }
        let min_x = values[0].as_f64()?;
        let min_y = values[1].as_f64()?;
        let max_x = values[2].as_f64()?;
        let max_y = values[3].as_f64()?;
        Some([min_x, min_y, max_x, max_y])
    });
    let Some(bounds) = bounds else {
        return Err(failure(
            "invalid_bounds",
            "bounds expects [min_x, min_y, max_x, max_y]",
        ));
    };
    if !bounds.iter().all(|v| v.is_finite()) || bounds[0] > bounds[2] || bounds[1] > bounds[3] {
        return Err(failure(
            "invalid_bounds",
            "bounds must be finite with min <= max",
        ));
    }
    Ok(Some(bounds))
}

fn parse_handles_filter(req: &Value) -> Option<Vec<Handle>> {
    if let Some(h) = req["handle"].as_str() {
        u64::from_str_radix(h.trim_start_matches("0x"), 16)
            .ok()
            .map(|val| vec![Handle::new(val)])
    } else if let Some(arr) = req["handles"].as_array() {
        let handles: Vec<_> = arr
            .iter()
            .filter_map(|v| {
                let s = v.as_str()?;
                u64::from_str_radix(s.trim_start_matches("0x"), 16)
                    .ok()
                    .map(Handle::new)
            })
            .collect();
        (!handles.is_empty()).then_some(handles)
    } else {
        None
    }
}

fn parse_types_filter(req: &Value) -> Option<Vec<String>> {
    if let Some(t) = req["type"].as_str() {
        Some(vec![t.to_string()])
    } else if let Some(arr) = req["types"].as_array() {
        let types: Vec<_> = arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        (!types.is_empty()).then_some(types)
    } else {
        None
    }
}

fn entity_block_name(doc: &CadDocument, entity_handle: Handle, owner: Handle) -> Option<String> {
    if let Some(record) = doc.block_records.iter().find(|r| r.handle == owner) {
        return Some(record.name.clone());
    }
    for record in doc.block_records.iter() {
        if record.entity_handles.contains(&entity_handle) {
            return Some(record.name.clone());
        }
    }
    None
}

impl OpenCADStudio {
    /// `text_search` — find text occurrences across active space entities, inserts,
    /// and block definitions with detailed machine-readable metadata.
    pub(crate) fn control_text_search(&self, req: &Value) -> Result<Value, Value> {
        let find = req["find"]
            .as_str()
            .or_else(|| req["search"].as_str())
            .unwrap_or("");
        if find.is_empty() {
            return Err(failure("missing_find", "Supply 'find' parameter"));
        }

        let match_case = req["match_case"].as_bool().unwrap_or(false);
        let whole_word = req["whole_word"].as_bool().unwrap_or(false);
        let ignore_accents = req["ignore_accents"].as_bool().unwrap_or(!match_case);
        let scope = req["scope"].as_str().unwrap_or("all");
        let layer_filter = req["layer"].as_str();
        let handles_filter = parse_handles_filter(req);
        let types_filter = parse_types_filter(req);
        let bounds_filter = parse_bounds_filter(req)?;
        let limit = req["limit"].as_u64().unwrap_or(500).min(5000) as usize;

        let matcher = TextMatcher::new(find.to_string(), match_case, whole_word, ignore_accents);
        let i = if let Some(doc_id) = req.get("document_id").and_then(|v| v.as_u64()) {
            self.tabs
                .iter()
                .position(|t| t.id == doc_id)
                .ok_or_else(|| failure("document_not_found", "Specified document_id not found"))?
        } else {
            self.active_tab
        };
        let scene = &self.tabs[i].scene;
        let doc = &scene.document;

        let mut matches = Vec::new();

        for entity in doc.entities() {
            let common = entity.common();
            let handle = common.handle;

            // Scope filter
            let in_active = scene.entity_belongs_to_active_space(handle);
            match scope {
                "active_space" | "model_space" if !in_active => continue,
                "blocks" if in_active => continue,
                _ => {}
            }

            // Layer filter
            if let Some(l) = layer_filter {
                if !common.layer.eq_ignore_ascii_case(l) {
                    continue;
                }
            }

            // Handle filter
            if let Some(ref h_list) = handles_filter {
                if !h_list.contains(&handle) {
                    continue;
                }
            }

            // Types filter
            if let Some(ref t_list) = types_filter {
                if !t_list
                    .iter()
                    .any(|t| crate::app::automation::entity_type_matches(entity, t))
                {
                    continue;
                }
            }

            // Bounds filter
            if let Some(b) = bounds_filter {
                let (min, max) = crate::scene::convert::tess::entity_bounds_in(doc, entity);
                if max[0] < b[0] || max[1] < b[1] || min[0] > b[2] || min[1] > b[3] {
                    continue;
                }
            }

            let block_name = entity_block_name(doc, handle, common.owner_handle);

            // Check standard text entities
            if let Some((raw_val, plain_text, pos)) = entity_text_info(entity, doc) {
                if matcher.matches_text_or_raw(&plain_text, &raw_val) {
                    let matched_text = find.to_string();
                    matches.push(json!({
                        "handle": format!("{:X}", handle.value()),
                        "type": crate::entities::names::ui_name(entity),
                        "layer": common.layer,
                        "space": if in_active { "active" } else { "definition" },
                        "block": block_name,
                        "position": pos,
                        "raw_value": raw_val,
                        "plain_text": plain_text,
                        "match_text": matched_text,
                    }));
                    if matches.len() >= limit {
                        break;
                    }
                }
            }

            // Check Insert attributes
            if let EntityType::Insert(insert) = entity {
                for (idx, attr) in insert.attributes.iter().enumerate() {
                    let raw = attr.get_value();
                    let plain = decode_dxf_escapes(raw);
                    if matcher.matches_text_or_raw(&plain, raw) {
                        matches.push(json!({
                            "handle": format!("{:X}", handle.value()),
                            "type": "Insert",
                            "layer": common.layer,
                            "space": if in_active { "active" } else { "definition" },
                            "block": insert.block_name,
                            "attribute_tag": attr.tag,
                            "attribute_index": idx,
                            "position": [insert.insert_point.x, insert.insert_point.y, insert.insert_point.z],
                            "raw_value": raw,
                            "plain_text": plain,
                            "match_text": find,
                        }));
                        if matches.len() >= limit {
                            break;
                        }
                    }
                }
                if matches.len() >= limit {
                    break;
                }
            }
        }

        let truncated = matches.len() >= limit;
        Ok(json!({
            "ok": true,
            "count": matches.len(),
            "truncated": truncated,
            "matches": matches,
        }))
    }

    /// `text_audit` — comprehensive text quality check, spell-check, and replacement dry-run.
    pub(crate) fn control_text_audit(&self, req: &Value) -> Result<Value, Value> {
        let i = if let Some(doc_id) = req.get("document_id").and_then(|v| v.as_u64()) {
            self.tabs
                .iter()
                .position(|t| t.id == doc_id)
                .ok_or_else(|| failure("document_not_found", "Specified document_id not found"))?
        } else {
            self.active_tab
        };

        let match_case = req["match_case"].as_bool().unwrap_or(false);
        let whole_word = req["whole_word"].as_bool().unwrap_or(false);
        let ignore_accents = req["ignore_accents"].as_bool().unwrap_or(!match_case);
        let scope = req["scope"].as_str().unwrap_or("all");
        let layer_filter = req["layer"].as_str();
        let handles_filter = parse_handles_filter(req);
        let types_filter = parse_types_filter(req);
        let bounds_filter = parse_bounds_filter(req)?;
        let limit = req["limit"].as_u64().unwrap_or(1000).min(10000) as usize;

        // Parse dry-run replacement pairs if supplied
        let mut pairs: Vec<(String, String)> = Vec::new();
        let pairs_val = req.get("pairs").or_else(|| req.get("dry_run_pairs"));
        if let Some(pair_array) = pairs_val.and_then(|v| v.as_array()) {
            for item in pair_array {
                let find = item["find"].as_str().unwrap_or("");
                let replace = item["replace"].as_str().unwrap_or("");
                if !find.is_empty() {
                    pairs.push((find.to_string(), replace.to_string()));
                }
            }
        }

        // Parse check terms if supplied
        let mut check_terms: Vec<String> = Vec::new();
        if let Some(terms_array) = req.get("check_terms").and_then(|v| v.as_array()) {
            for item in terms_array {
                if let Some(t) = item.as_str() {
                    if !t.is_empty() {
                        check_terms.push(t.to_string());
                    }
                }
            }
        }

        // Parse dictionary if supplied
        let mut dictionary: Vec<String> = Vec::new();
        if let Some(dict_array) = req.get("dictionary").and_then(|v| v.as_array()) {
            for item in dict_array {
                if let Some(w) = item.as_str() {
                    if !w.is_empty() {
                        dictionary.push(w.to_string());
                    }
                }
            }
        }

        let pair_matchers: Vec<(TextMatcher, String)> = pairs
            .iter()
            .map(|(f, r)| {
                (
                    TextMatcher::new(f.clone(), match_case, whole_word, ignore_accents),
                    r.clone(),
                )
            })
            .collect();

        let check_matchers: Vec<TextMatcher> = check_terms
            .iter()
            .map(|t| TextMatcher::new(t.clone(), match_case, whole_word, ignore_accents))
            .collect();

        let dict_matchers: Vec<TextMatcher> = dictionary
            .iter()
            .map(|w| TextMatcher::new(w.clone(), false, true, true))
            .collect();

        let use_system_speller = req["system_spellcheck"]
            .as_bool()
            .or_else(|| req["use_system_speller"].as_bool())
            .or_else(|| req["system_speller"].as_bool())
            .unwrap_or(false);
        let language_param = req["language"].as_str();
        let want_suggestions = req["suggest"].as_bool().unwrap_or(true);

        let system_speller = if use_system_speller {
            Some(super::spellcheck::SystemSpeller::new(language_param))
        } else {
            None
        };

        let mut pair_hit_counts = vec![0usize; pair_matchers.len()];
        let mut check_hit_counts = vec![0usize; check_matchers.len()];

        let scene = &self.tabs[i].scene;
        let doc = &scene.document;

        let mut entities_scanned = 0usize;
        let mut entities_matched = 0usize;
        let mut total_occurrences = 0usize;
        let mut simulated_changes = Vec::new();
        let mut suspect_matches = Vec::new();
        let mut unrecognized_words = Vec::new();

        for entity in doc.entities() {
            let common = entity.common();
            let handle = common.handle;

            let in_active = scene.entity_belongs_to_active_space(handle);
            match scope {
                "active_space" | "model_space" if !in_active => continue,
                "blocks" if in_active => continue,
                _ => {}
            }

            if let Some(l) = layer_filter {
                if !common.layer.eq_ignore_ascii_case(l) {
                    continue;
                }
            }

            if let Some(ref h_list) = handles_filter {
                if !h_list.contains(&handle) {
                    continue;
                }
            }

            if let Some(ref t_list) = types_filter {
                if !t_list
                    .iter()
                    .any(|t| crate::app::automation::entity_type_matches(entity, t))
                {
                    continue;
                }
            }

            if let Some(b) = bounds_filter {
                let (min, max) = crate::scene::convert::tess::entity_bounds_in(doc, entity);
                if max[0] < b[0] || max[1] < b[1] || min[0] > b[2] || min[1] > b[3] {
                    continue;
                }
            }

            let Some((raw_val, plain_text, pos)) = entity_text_info(entity, doc) else {
                continue;
            };

            entities_scanned += 1;
            let mut entity_had_match = false;

            // 1. Dry run pairs simulation
            if !pair_matchers.is_empty() {
                match entity {
                    EntityType::MText(_) => {
                        let mut cur_raw = raw_val.clone();
                        for (idx, (matcher, replacement)) in pair_matchers.iter().enumerate() {
                            let (new_raw, count) =
                                replace_mtext_safe(&cur_raw, matcher, replacement, true);
                            if count > 0 {
                                pair_hit_counts[idx] += count;
                                total_occurrences += count;
                                entity_had_match = true;
                                if simulated_changes.len() < limit {
                                    simulated_changes.push(json!({
                                        "handle": format!("{:X}", handle.value()),
                                        "type": "MText",
                                        "layer": common.layer,
                                        "position": pos,
                                        "find": pairs[idx].0,
                                        "replace": pairs[idx].1,
                                        "before": cur_raw,
                                        "after": new_raw.clone(),
                                        "replaced": count,
                                    }));
                                }
                                cur_raw = new_raw;
                            }
                        }
                    }
                    _ => {
                        let mut cur_text = plain_text.clone();
                        for (idx, (matcher, replacement)) in pair_matchers.iter().enumerate() {
                            let (new_text, count) =
                                matcher.replace_all_in(&cur_text, replacement, true);
                            if count > 0 {
                                pair_hit_counts[idx] += count;
                                total_occurrences += count;
                                entity_had_match = true;
                                if simulated_changes.len() < limit {
                                    simulated_changes.push(json!({
                                        "handle": format!("{:X}", handle.value()),
                                        "type": crate::entities::names::ui_name(entity),
                                        "layer": common.layer,
                                        "position": pos,
                                        "find": pairs[idx].0,
                                        "replace": pairs[idx].1,
                                        "before": cur_text,
                                        "after": new_text.clone(),
                                        "replaced": count,
                                    }));
                                }
                                cur_text = new_text;
                            }
                        }
                    }
                }
            }

            // 2. Check terms
            for (idx, matcher) in check_matchers.iter().enumerate() {
                if matcher.matches_text_or_raw(&plain_text, &raw_val) {
                    check_hit_counts[idx] += 1;
                    entity_had_match = true;
                    if suspect_matches.len() < limit {
                        suspect_matches.push(json!({
                            "handle": format!("{:X}", handle.value()),
                            "type": crate::entities::names::ui_name(entity),
                            "layer": common.layer,
                            "position": pos,
                            "term": check_terms[idx],
                            "plain_text": plain_text,
                        }));
                    }
                }
            }

            // 3. Dictionary & System Spell-check
            if !dict_matchers.is_empty() || system_speller.is_some() {
                for word in plain_text.split(|c: char| !c.is_alphabetic()) {
                    let w = word.trim();
                    if w.len() >= 3 {
                        let in_agent_dict = dict_matchers.iter().any(|dm| dm.matches(w));
                        if in_agent_dict {
                            continue;
                        }

                        let (is_system_valid, suggestions) = if let Some(ref speller) = system_speller {
                            speller.check_word(w, want_suggestions)
                        } else {
                            (false, Vec::new())
                        };

                        if !is_system_valid {
                            entity_had_match = true;
                            if unrecognized_words.len() < limit
                                && !unrecognized_words
                                    .iter()
                                    .any(|item: &Value| item["word"].as_str() == Some(w))
                            {
                                let mut item = json!({
                                    "word": w,
                                    "handle": format!("{:X}", handle.value()),
                                    "position": pos,
                                });
                                if !suggestions.is_empty() {
                                    item["suggestions"] = json!(suggestions);
                                }
                                unrecognized_words.push(item);
                            }
                        }
                    }
                }
            }

            // Check Insert attributes
            if let EntityType::Insert(insert) = entity {
                for attr in &insert.attributes {
                    let raw = attr.get_value();
                    let plain = decode_dxf_escapes(raw);

                    for (idx, (matcher, replacement)) in pair_matchers.iter().enumerate() {
                        let (new_text, count) =
                            matcher.replace_all_in(&plain, replacement, true);
                        if count > 0 {
                            pair_hit_counts[idx] += count;
                            total_occurrences += count;
                            entity_had_match = true;
                            if simulated_changes.len() < limit {
                                simulated_changes.push(json!({
                                    "handle": format!("{:X}", handle.value()),
                                    "type": "Insert",
                                    "attribute_tag": attr.tag,
                                    "layer": common.layer,
                                    "position": [insert.insert_point.x, insert.insert_point.y, insert.insert_point.z],
                                    "find": pairs[idx].0,
                                    "replace": pairs[idx].1,
                                    "before": plain.clone(),
                                    "after": new_text,
                                    "replaced": count,
                                }));
                            }
                        }
                    }

                    for (idx, matcher) in check_matchers.iter().enumerate() {
                        if matcher.matches_text_or_raw(&plain, raw) {
                            check_hit_counts[idx] += 1;
                            entity_had_match = true;
                            if suspect_matches.len() < limit {
                                suspect_matches.push(json!({
                                    "handle": format!("{:X}", handle.value()),
                                    "type": "Insert",
                                    "attribute_tag": attr.tag,
                                    "layer": common.layer,
                                    "position": [insert.insert_point.x, insert.insert_point.y, insert.insert_point.z],
                                    "term": check_terms[idx],
                                    "plain_text": plain,
                                }));
                            }
                        }
                    }

                    if !dict_matchers.is_empty() || system_speller.is_some() {
                        for word in plain.split(|c: char| !c.is_alphabetic()) {
                            let w = word.trim();
                            if w.len() >= 3 {
                                let in_agent_dict = dict_matchers.iter().any(|dm| dm.matches(w));
                                if in_agent_dict {
                                    continue;
                                }

                                let (is_system_valid, suggestions) = if let Some(ref speller) = system_speller {
                                    speller.check_word(w, want_suggestions)
                                } else {
                                    (false, Vec::new())
                                };

                                if !is_system_valid {
                                    entity_had_match = true;
                                    if unrecognized_words.len() < limit
                                        && !unrecognized_words
                                            .iter()
                                            .any(|item: &Value| item["word"].as_str() == Some(w))
                                    {
                                        let mut item = json!({
                                            "word": w,
                                            "handle": format!("{:X}", handle.value()),
                                            "attribute_tag": attr.tag,
                                            "position": [insert.insert_point.x, insert.insert_point.y, insert.insert_point.z],
                                        });
                                        if !suggestions.is_empty() {
                                            item["suggestions"] = json!(suggestions);
                                        }
                                        unrecognized_words.push(item);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if entity_had_match {
                entities_matched += 1;
            }
        }

        // Unmatched pairs / queries
        let unmatched_pairs: Vec<String> = pair_hit_counts
            .iter()
            .enumerate()
            .filter(|(_, &count)| count == 0)
            .map(|(idx, _)| pairs[idx].0.clone())
            .collect();

        let unmatched_check_terms: Vec<String> = check_hit_counts
            .iter()
            .enumerate()
            .filter(|(_, &count)| count == 0)
            .map(|(idx, _)| check_terms[idx].clone())
            .collect();

        let speller_info = if let Some(ref speller) = system_speller {
            json!({
                "enabled": true,
                "available": speller.is_available(),
                "backend": speller.backend_name(),
                "language": speller.language(),
            })
        } else {
            json!({
                "enabled": false,
                "available": false,
                "backend": "none",
                "language": null,
            })
        };

        Ok(json!({
            "ok": true,
            "summary": {
                "entities_scanned": entities_scanned,
                "entities_matched": entities_matched,
                "occurrences_matched": total_occurrences,
                "pairs_total": pairs.len(),
                "pairs_unmatched": unmatched_pairs.len(),
                "check_terms_total": check_terms.len(),
                "check_terms_found": check_terms.len() - unmatched_check_terms.len(),
                "unrecognized_words_count": unrecognized_words.len(),
            },
            "system_speller": speller_info,
            "unmatched_pairs": unmatched_pairs,
            "unmatched_check_terms": unmatched_check_terms,
            "simulated_changes": simulated_changes,
            "suspect_matches": suspect_matches,
            "unrecognized_words": unrecognized_words,
        }))
    }

    /// Execute text replace across candidates with single-transaction undo support.
    pub(crate) fn execute_text_replace(
        &mut self,
        req: &Value,
        push_undo: bool,
    ) -> Result<Value, Value> {
        let i = if let Some(doc_id) = req.get("document_id").and_then(|v| v.as_u64()) {
            self.tabs
                .iter()
                .position(|t| t.id == doc_id)
                .ok_or_else(|| failure("document_not_found", "Specified document_id not found"))?
        } else {
            self.active_tab
        };

        // Parse search/replace pairs
        let mut pairs: Vec<(String, String)> = Vec::new();
        if let Some(pair_array) = req["pairs"].as_array() {
            for item in pair_array {
                let find = item["find"].as_str().unwrap_or("");
                let replace = item["replace"].as_str().unwrap_or("");
                if !find.is_empty() {
                    pairs.push((find.to_string(), replace.to_string()));
                }
            }
        } else if let Some(find) = req["find"].as_str() {
            if !find.is_empty() {
                let replace = req["replace"].as_str().unwrap_or("");
                pairs.push((find.to_string(), replace.to_string()));
            }
        }

        if pairs.is_empty() {
            return Err(failure(
                "missing_find",
                "Supply 'find'/'replace' or 'pairs' for text_replace",
            ));
        }

        let match_case = req["match_case"].as_bool().unwrap_or(false);
        let whole_word = req["whole_word"].as_bool().unwrap_or(false);
        let ignore_accents = req["ignore_accents"].as_bool().unwrap_or(!match_case);
        let dry_run = req["dry_run"].as_bool().unwrap_or(false);
        let replace_all = req["replace_all"].as_bool().unwrap_or(true);
        let scope = req["scope"].as_str().unwrap_or("all");
        let layer_filter = req["layer"].as_str();
        let handles_filter = parse_handles_filter(req);
        let types_filter = parse_types_filter(req);
        let bounds_filter = parse_bounds_filter(req)?;

        let matchers: Vec<(TextMatcher, String)> = pairs
            .into_iter()
            .map(|(f, r)| (TextMatcher::new(f, match_case, whole_word, ignore_accents), r))
            .collect();

        if push_undo && !dry_run {
            self.push_undo_snapshot(i, "TEXT REPLACE");
        }

        let mut total_replaced = 0usize;
        let mut changed_handles = Vec::new();
        let mut changes_log = Vec::new();

        // Collect all entity handles that match filters
        let candidate_handles: Vec<Handle> = {
            let scene = &self.tabs[i].scene;
            let doc = &scene.document;
            let mut candidates = Vec::new();
            for entity in doc.entities() {
                let common = entity.common();
                let handle = common.handle;

                let in_active = scene.entity_belongs_to_active_space(handle);
                match scope {
                    "active_space" | "model_space" if !in_active => continue,
                    "blocks" if in_active => continue,
                    _ => {}
                }

                if let Some(l) = layer_filter {
                    if !common.layer.eq_ignore_ascii_case(l) {
                        continue;
                    }
                }

                if let Some(ref h_list) = handles_filter {
                    if !h_list.contains(&handle) {
                        continue;
                    }
                }

                if let Some(ref t_list) = types_filter {
                    if !t_list
                        .iter()
                        .any(|t| crate::app::automation::entity_type_matches(entity, t))
                    {
                        continue;
                    }
                }

                if let Some(b) = bounds_filter {
                    let (min, max) = crate::scene::convert::tess::entity_bounds_in(doc, entity);
                    if max[0] < b[0] || max[1] < b[1] || min[0] > b[2] || min[1] > b[3] {
                        continue;
                    }
                }

                if scene.is_layer_locked(handle) {
                    continue;
                }

                candidates.push(handle);
            }
            candidates
        };

        // Mutate matching entities
        let doc = &mut self.tabs[i].scene.document;
        for handle in candidate_handles {
            let Some(entity) = doc.get_entity_mut(handle) else {
                continue;
            };

            let mut entity_replaced = 0;
            let mut before_val = None;
            let mut after_val = None;

            match entity {
                EntityType::Text(text) => {
                    for (matcher, replacement) in &matchers {
                        let (new_val, count) =
                            matcher.replace_all_in(&text.value, replacement, replace_all);
                        if count > 0 {
                            if before_val.is_none() {
                                before_val = Some(text.value.clone());
                            }
                            if !dry_run {
                                text.value = new_val.clone();
                            }
                            entity_replaced += count;
                            after_val = Some(new_val);
                        } else {
                            // Check if matching decoded DXF escapes
                            let decoded = decode_dxf_escapes(&text.value);
                            if matcher.matches(&decoded) {
                                let (new_val, c) =
                                    matcher.replace_all_in(&decoded, replacement, replace_all);
                                if c > 0 {
                                    if before_val.is_none() {
                                        before_val = Some(text.value.clone());
                                    }
                                    if !dry_run {
                                        text.value = new_val.clone();
                                    }
                                    entity_replaced += c;
                                    after_val = Some(new_val);
                                }
                            }
                        }
                    }
                }
                EntityType::MText(mtext) => {
                    for (matcher, replacement) in &matchers {
                        let (new_val, count) =
                            replace_mtext_safe(&mtext.value, matcher, replacement, replace_all);
                        if count > 0 {
                            if before_val.is_none() {
                                before_val = Some(mtext.value.clone());
                            }
                            if !dry_run {
                                mtext.value = new_val.clone();
                            }
                            entity_replaced += count;
                            after_val = Some(new_val);
                        }
                    }
                }
                EntityType::AttributeDefinition(ad) => {
                    for (matcher, replacement) in &matchers {
                        let (new_val, count) =
                            matcher.replace_all_in(&ad.default_value, replacement, replace_all);
                        if count > 0 {
                            if before_val.is_none() {
                                before_val = Some(ad.default_value.clone());
                            }
                            if !dry_run {
                                ad.default_value = new_val.clone();
                            }
                            entity_replaced += count;
                            after_val = Some(new_val);
                        }
                    }
                }
                EntityType::AttributeEntity(ae) => {
                    for (matcher, replacement) in &matchers {
                        let (new_val, count) =
                            matcher.replace_all_in(ae.get_value(), replacement, replace_all);
                        if count > 0 {
                            if before_val.is_none() {
                                before_val = Some(ae.get_value().to_string());
                            }
                            if !dry_run {
                                ae.set_value(new_val.clone());
                            }
                            entity_replaced += count;
                            after_val = Some(new_val);
                        }
                    }
                }
                EntityType::Dimension(dim) => {
                    let base = dim.base_mut();
                    if !base.text.is_empty() {
                        for (matcher, replacement) in &matchers {
                            let (new_val, count) =
                                matcher.replace_all_in(&base.text, replacement, replace_all);
                            if count > 0 {
                                if before_val.is_none() {
                                    before_val = Some(base.text.clone());
                                }
                                if !dry_run {
                                    base.text = new_val.clone();
                                }
                                entity_replaced += count;
                                after_val = Some(new_val);
                            }
                        }
                    }
                }
                EntityType::Insert(insert) => {
                    for attr in &mut insert.attributes {
                        for (matcher, replacement) in &matchers {
                            let (new_val, count) =
                                matcher.replace_all_in(attr.get_value(), replacement, replace_all);
                            if count > 0 {
                                let old_val = attr.get_value().to_string();
                                if !dry_run {
                                    attr.set_value(new_val.clone());
                                }
                                total_replaced += count;
                                changes_log.push(json!({
                                    "handle": format!("{:X}", handle.value()),
                                    "type": "Insert",
                                    "attribute_tag": attr.tag,
                                    "before": old_val,
                                    "after": new_val,
                                    "replaced": count,
                                }));
                                if !changed_handles.contains(&handle) {
                                    changed_handles.push(handle);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }

            if entity_replaced > 0 {
                total_replaced += entity_replaced;
                changes_log.push(json!({
                    "handle": format!("{:X}", handle.value()),
                    "type": crate::entities::names::ui_name(entity),
                    "before": before_val,
                    "after": after_val,
                    "replaced": entity_replaced,
                }));
                if !changed_handles.contains(&handle) {
                    changed_handles.push(handle);
                }
            }
        }

        if total_replaced == 0 {
            if push_undo && !dry_run {
                self.discard_last_undo_entry(i);
            }
            return Ok(json!({
                "ok": true,
                "dry_run": dry_run,
                "replaced": 0,
                "entities_changed": 0,
                "changes": [],
            }));
        }

        if !dry_run {
            // Refresh scene caches and dirty flags
            self.invalidate_property_targets(i, &changed_handles);
            self.tabs[i].scene.bump_geometry();
            self.tabs[i].dirty = true;
            self.refresh_properties();

            self.command_line.push_output(
                format!(
                    "TEXT REPLACE: replaced {total_replaced} occurrence(s) in {} object(s).",
                    changed_handles.len()
                )
                .as_str(),
            );
        }

        Ok(json!({
            "ok": true,
            "dry_run": dry_run,
            "replaced": total_replaced,
            "entities_changed": changed_handles.len(),
            "changes": changes_log,
        }))
    }

    /// Bridge for `control_request` mutating op `"text_replace"`.
    pub(super) fn control_text_replace(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let result = self.execute_text_replace(req, true)?;
        self.set_control_result(result);
        Ok(Task::none())
    }

    /// Headless automation text search.
    pub(crate) fn automation_text_search(&self, req: &Value) -> Value {
        self.control_text_search(req).unwrap_or_else(|e| e)
    }

    /// Headless automation text audit.
    pub(crate) fn automation_text_audit(&self, req: &Value) -> Value {
        self.control_text_audit(req).unwrap_or_else(|e| e)
    }

    /// Headless automation text replace.
    pub(crate) fn automation_text_replace(&mut self, req: &Value) -> Value {
        self.execute_text_replace(req, true).unwrap_or_else(|e| e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_dxf_escapes() {
        assert_eq!(decode_dxf_escapes("CIRCUITS"), "CIRCUITS");
        assert_eq!(decode_dxf_escapes("R\\U+00C9SERVOIR"), "RÉSERVOIR");
        assert_eq!(decode_dxf_escapes("r\\u+00e9servoir"), "réservoir");
        assert_eq!(decode_dxf_escapes("Angle %%d"), "Angle °");
        assert_eq!(decode_dxf_escapes("Tolerance %%p0.05"), "Tolerance ±0.05");
        assert_eq!(decode_dxf_escapes("Diameter %%c50"), "Diameter ∅50");
        assert_eq!(decode_dxf_escapes("100%%%%"), "100%");
        // Windows-1252 / ISO-8859-1 code decoding
        assert_eq!(decode_dxf_escapes("Chaudi%%232re"), "Chaudière");
        assert_eq!(decode_dxf_escapes("St%%233 EVERWASH"), "Sté EVERWASH");
    }

    #[test]
    fn test_text_matcher_case_and_word_boundaries() {
        let tm_ci = TextMatcher::new("bar".into(), false, false, false);
        assert!(tm_ci.matches("BAROMETRE"));
        assert!(tm_ci.matches("bar"));
        assert!(tm_ci.matches("BAR"));

        let tm_ww = TextMatcher::new("bar".into(), false, true, false);
        assert!(!tm_ww.matches("BAROMETRE"));
        assert!(!tm_ww.matches("MINIBAR"));
        assert!(tm_ww.matches("BAR"));
        assert!(tm_ww.matches("P = 10 BAR"));
        assert!(tm_ww.matches("BAR;"));

        let tm_cs = TextMatcher::new("Bar".into(), true, false, false);
        assert!(!tm_cs.matches("BAR"));
        assert!(tm_cs.matches("Barometer"));

        let (replaced, count) = tm_ww.replace_all_in("10 BAR ET 20 BAROMETRE", "PSI", true);
        assert_eq!(count, 1);
        assert_eq!(replaced, "10 PSI ET 20 BAROMETRE");
    }

    #[test]
    fn test_text_matcher_accent_insensitive() {
        let tm_acc = TextMatcher::new("reservoir".into(), false, false, true);
        assert!(tm_acc.matches("RÉSERVOIR"));
        assert!(tm_acc.matches("Réservoir"));
        assert!(tm_acc.matches("reservoir"));

        let tm_ch = TextMatcher::new("chaudiere".into(), false, false, true);
        assert!(tm_ch.matches("Chaudière 1"));
        let (replaced, count) = tm_ch.replace_all_in("Chaudière 1 et Chaudière 2", "Boiler", true);
        assert_eq!(count, 2);
        assert_eq!(replaced, "Boiler 1 et Boiler 2");

        let tm_bache = TextMatcher::new("bache a eau".into(), false, false, true);
        assert!(tm_bache.matches("Bâche à eau 2"));
    }

    #[test]
    fn test_replace_mtext_safe_preserves_formatting() {
        let raw = "{\\fCentury Gothic|b0|i0|c0|p34;V15-CUVE EAU NON TRAITE}";
        let matcher = TextMatcher::new("NON TRAITE".into(), false, false, false);
        let (replaced, count) = replace_mtext_safe(raw, &matcher, "NON TRAITÉE", true);
        assert_eq!(count, 1);
        assert!(replaced.contains("NON TRAITÉE"));
        assert!(replaced.contains("Century Gothic"));
        assert!(!replaced.contains("NON TRAITE}"));
    }

    #[test]
    fn test_replace_mtext_multiline() {
        // Test 1: Plain LF query matching raw \P
        let raw1 = "Loge \\Pgardien";
        let matcher1 = TextMatcher::new("Loge \ngardien".into(), false, false, true);
        let (replaced1, count1) = replace_mtext_safe(raw1, &matcher1, "Security \nGuard Booth", true);
        assert_eq!(count1, 1);
        assert_eq!(replaced1, "Security \\PGuard Booth");

        // Test 2: Multiline with complex formatting codes preserved
        let raw2 = "\\pxqc;{\\fCentury Gothic|b1|i0|c0|p34;ACCÈS ET SORTIE\\PMARCHANDISES}";
        let matcher2 = TextMatcher::new("ACCÈS ET SORTIE\nMARCHANDISES".into(), false, false, true);
        let (replaced2, count2) = replace_mtext_safe(raw2, &matcher2, "GOODS ACCESS AND EXIT", true);
        assert_eq!(count2, 1);
        assert!(replaced2.contains("GOODS ACCESS AND EXIT"));
        assert!(replaced2.contains("\\pxqc;"));
        assert!(replaced2.contains("Century Gothic"));

        // Test 3: Multiline with paragraph properties and flexible whitespace
        let raw3 = "{\\pqc;Office \\PWAREHOUSE}";
        let matcher3 = TextMatcher::new("Office \nWAREHOUSE".into(), false, false, true);
        let (replaced3, count3) = replace_mtext_safe(raw3, &matcher3, "WAREHOUSE OFFICE", true);
        assert_eq!(count3, 1);
        assert_eq!(replaced3, "{\\pqc;WAREHOUSE OFFICE}");
    }
}

