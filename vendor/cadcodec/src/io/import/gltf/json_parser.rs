//! Minimal JSON value parser for glTF import.
//!
//! Supports the subset of JSON needed for glTF: objects, arrays, strings,
//! numbers, booleans, and null.  Not a general-purpose parser.

use crate::error::{DxfError, Result};

/// A JSON value.
#[derive(Debug, Clone)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// Parse a JSON string into a [`JsonValue`].
    pub fn parse(input: &str) -> Result<JsonValue> {
        let bytes = input.as_bytes();
        let (val, _) = parse_value(bytes, skip_ws(bytes, 0), 0)?;
        Ok(val)
    }

    /// Get a value by object key.
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Get a value by array index.
    pub fn at(&self, index: usize) -> Option<&JsonValue> {
        match self {
            JsonValue::Array(arr) => arr.get(index),
            _ => None,
        }
    }

    /// Get as string slice.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Get as f64.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Get as usize.
    pub fn as_usize(&self) -> Option<usize> {
        match self {
            JsonValue::Number(n) => {
                let v = *n;
                if v >= 0.0 && v == (v as usize as f64) {
                    Some(v as usize)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get as array.
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(arr) => Some(arr),
            _ => None,
        }
    }

    /// Get as object pairs.
    pub fn as_object(&self) -> Option<&[(String, JsonValue)]> {
        match self {
            JsonValue::Object(pairs) => Some(pairs),
            _ => None,
        }
    }

    /// Get as bool.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

const MAX_DEPTH: usize = 128;

fn err(msg: &str, pos: usize) -> DxfError {
    DxfError::ImportError(format!("JSON parse error at byte {}: {}", pos, msg))
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

fn parse_value(b: &[u8], i: usize, depth: usize) -> Result<(JsonValue, usize)> {
    if i >= b.len() {
        return Err(err("unexpected end of input", i));
    }
    if depth > MAX_DEPTH {
        return Err(err("JSON depth limit exceeded", i));
    }
    match b[i] {
        b'"' => parse_string(b, i),
        b'{' => parse_object(b, i, depth + 1),
        b'[' => parse_array(b, i, depth + 1),
        b't' | b'f' => parse_bool(b, i),
        b'n' => parse_null(b, i),
        _ => parse_number(b, i),
    }
}

fn parse_string(b: &[u8], i: usize) -> Result<(JsonValue, usize)> {
    let (s, end) = parse_string_raw(b, i)?;
    Ok((JsonValue::Str(s), end))
}

fn parse_string_raw(b: &[u8], i: usize) -> Result<(String, usize)> {
    if b[i] != b'"' {
        return Err(err("expected '\"'", i));
    }
    let mut j = i + 1;
    let mut s = String::new();
    while j < b.len() {
        match b[j] {
            b'"' => return Ok((s, j + 1)),
            b'\\' => {
                j += 1;
                if j >= b.len() {
                    return Err(err("unexpected end in string escape", j));
                }
                match b[j] {
                    b'"' => s.push('"'),
                    b'\\' => s.push('\\'),
                    b'/' => s.push('/'),
                    b'n' => s.push('\n'),
                    b'r' => s.push('\r'),
                    b't' => s.push('\t'),
                    b'b' => s.push('\u{08}'),
                    b'f' => s.push('\u{0C}'),
                    b'u' => {
                        // \uXXXX
                        if j + 4 >= b.len() {
                            return Err(err("incomplete unicode escape", j));
                        }
                        let hex = std::str::from_utf8(&b[j + 1..j + 5])
                            .map_err(|_| err("invalid unicode hex", j))?;
                        let cp = u32::from_str_radix(hex, 16)
                            .map_err(|_| err("invalid unicode hex", j))?;
                        if let Some(c) = char::from_u32(cp) {
                            s.push(c);
                        }
                        j += 4;
                    }
                    _ => {
                        s.push('\\');
                        s.push(b[j] as char);
                    }
                }
            }
            _ => {
                // Fast path: scan for end of simple substring
                let start = j;
                while j < b.len() && b[j] != b'"' && b[j] != b'\\' {
                    j += 1;
                }
                s.push_str(
                    std::str::from_utf8(&b[start..j])
                        .map_err(|_| err("invalid UTF-8 in string", start))?,
                );
                continue;
            }
        }
        j += 1;
    }
    Err(err("unterminated string", i))
}

fn parse_number(b: &[u8], i: usize) -> Result<(JsonValue, usize)> {
    let start = i;
    let mut j = i;
    if j < b.len() && b[j] == b'-' {
        j += 1;
    }
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    if j < b.len() && b[j] == b'.' {
        j += 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
    }
    if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
        j += 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
    }
    let s = std::str::from_utf8(&b[start..j]).map_err(|_| err("invalid number", start))?;
    let n: f64 = s.parse().map_err(|_| err("invalid number", start))?;
    Ok((JsonValue::Number(n), j))
}

fn parse_bool(b: &[u8], i: usize) -> Result<(JsonValue, usize)> {
    if b[i..].starts_with(b"true") {
        Ok((JsonValue::Bool(true), i + 4))
    } else if b[i..].starts_with(b"false") {
        Ok((JsonValue::Bool(false), i + 5))
    } else {
        Err(err("expected boolean", i))
    }
}

fn parse_null(b: &[u8], i: usize) -> Result<(JsonValue, usize)> {
    if b[i..].starts_with(b"null") {
        Ok((JsonValue::Null, i + 4))
    } else {
        Err(err("expected null", i))
    }
}

fn parse_array(b: &[u8], i: usize, depth: usize) -> Result<(JsonValue, usize)> {
    let mut j = skip_ws(b, i + 1);
    let mut arr = Vec::new();
    if j < b.len() && b[j] == b']' {
        return Ok((JsonValue::Array(arr), j + 1));
    }
    loop {
        let (val, next) = parse_value(b, j, depth)?;
        arr.push(val);
        j = skip_ws(b, next);
        if j >= b.len() {
            return Err(err("unterminated array", i));
        }
        if b[j] == b']' {
            return Ok((JsonValue::Array(arr), j + 1));
        }
        if b[j] != b',' {
            return Err(err("expected ',' or ']' in array", j));
        }
        j = skip_ws(b, j + 1);
    }
}

fn parse_object(b: &[u8], i: usize, depth: usize) -> Result<(JsonValue, usize)> {
    let mut j = skip_ws(b, i + 1);
    let mut pairs = Vec::new();
    if j < b.len() && b[j] == b'}' {
        return Ok((JsonValue::Object(pairs), j + 1));
    }
    loop {
        let (key, after_key) = parse_string_raw(b, j)?;
        j = skip_ws(b, after_key);
        if j >= b.len() || b[j] != b':' {
            return Err(err("expected ':' in object", j));
        }
        j = skip_ws(b, j + 1);
        let (val, after_val) = parse_value(b, j, depth)?;
        pairs.push((key, val));
        j = skip_ws(b, after_val);
        if j >= b.len() {
            return Err(err("unterminated object", i));
        }
        if b[j] == b'}' {
            return Ok((JsonValue::Object(pairs), j + 1));
        }
        if b[j] != b',' {
            return Err(err("expected ',' or '}' in object", j));
        }
        j = skip_ws(b, j + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_number() {
        let v = JsonValue::parse("42").unwrap();
        assert_eq!(v.as_f64(), Some(42.0));
    }

    #[test]
    fn test_parse_string() {
        let v = JsonValue::parse(r#""hello""#).unwrap();
        assert_eq!(v.as_str(), Some("hello"));
    }

    #[test]
    fn test_parse_object() {
        let v = JsonValue::parse(r#"{"a": 1, "b": "two"}"#).unwrap();
        assert_eq!(v.get("a").unwrap().as_f64(), Some(1.0));
        assert_eq!(v.get("b").unwrap().as_str(), Some("two"));
    }

    #[test]
    fn test_parse_array() {
        let v = JsonValue::parse("[1, 2, 3]").unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_f64(), Some(1.0));
    }

    #[test]
    fn test_parse_nested() {
        let v = JsonValue::parse(r#"{"meshes": [{"name": "m", "primitives": []}]}"#).unwrap();
        let meshes = v.get("meshes").unwrap().as_array().unwrap();
        assert_eq!(meshes[0].get("name").unwrap().as_str(), Some("m"));
    }

    #[test]
    fn test_parse_recursion_limit() {
        let mut s = String::new();
        for _ in 0..200 {
            s.push('[');
        }
        for _ in 0..200 {
            s.push(']');
        }
        let res = JsonValue::parse(&s);
        assert!(res.is_err());
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("depth limit exceeded"),
            "Unexpected error: {}",
            err
        );
    }
}
