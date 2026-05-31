//! OpenCADStudio linetype catalog — loaded from `assets/linetypes/OpenCADStudio.lin`.
//!
//! Call [`populate_document`] to add all standard linetypes to a new document.
//! Linetypes that already exist in the document are skipped.
//!
//! [`complex_lt`] returns the complex segment catalog for CPU-side rendering.

use std::collections::HashMap;
use std::sync::OnceLock;

use acadrust::tables::linetype::{LineType, LineTypeElement};
use acadrust::{CadDocument, TableEntry};

// ── Complex linetype types ────────────────────────────────────────────────

/// One element in a complex linetype pattern.
#[derive(Clone, Debug)]
pub enum LtSegment {
    /// Draw a dash of this length (world units).
    Dash(f32),
    /// Skip a gap of this length (world units).
    Space(f32),
    /// Draw a dot (zero-length element, rendered as a tiny mark).
    Dot,
    /// Draw a shape at the current pen position. Embedded SHAPE elements are
    /// no longer rendered (the LFF font set ships no shape file).
    Shape {
        /// Shape name (retained for round-tripping; not drawn).
        name: String,
        /// X offset along the linetype direction (can be negative).
        x: f32,
        /// Y offset perpendicular to the linetype direction.
        y: f32,
        /// Scale factor applied to the CXF coordinates.
        scale: f32,
        /// Optional rotation in degrees (0 = along the line).
        rot_deg: f32,
    },
    /// Draw a text string at the current pen position.
    Text {
        /// The string to render.
        text: String,
        /// Text style / font name.
        style: String,
        /// X offset along the linetype direction.
        x: f32,
        /// Y offset perpendicular to the linetype direction.
        y: f32,
        /// Height scale factor.
        scale: f32,
        /// Rotation in degrees (0 = along the line).
        rot_deg: f32,
    },
}

/// A complex linetype — ordered elements for one pattern repeat.
#[derive(Clone, Debug)]
pub struct ComplexLt {
    pub segments: Vec<LtSegment>,
}

static COMPLEX_CATALOG: OnceLock<HashMap<String, ComplexLt>> = OnceLock::new();

/// Look up a complex linetype by name (case-insensitive).
/// Returns `None` for simple (dash-only) linetypes or unknown names.
pub fn complex_lt(name: &str) -> Option<&'static ComplexLt> {
    COMPLEX_CATALOG
        .get_or_init(|| parse_complex(LIN_SOURCE))
        .get(&name.to_ascii_uppercase())
}

const LIN_SOURCE: &str = include_str!("../assets/linetypes/OpenCADStudio.lin");

// ── Pattern art extraction ────────────────────────────────────────────────

/// Extract the ASCII-art portion of a LIN description.
///
/// LIN descriptions look like `"Dashed __ __ __ __"` or `"ISO dot . . . ."`.
/// This function returns the trailing pattern string (e.g. `"__ __ __ __"`).
pub fn extract_pattern(desc: &str) -> String {
    // Patterns that use dashes start with `__`.
    if let Some(pos) = desc.find("__") {
        let start = desc[..pos].rfind(' ').map(|p| p + 1).unwrap_or(0);
        return desc[start..].trim().to_string();
    }
    // Dot-only patterns: look for `. .` sequence.
    if let Some(pos) = desc.find(". .") {
        let start = desc[..pos].rfind(' ').map(|p| p + 1).unwrap_or(0);
        return desc[start..].trim().to_string();
    }
    // Fallback: return the whole description.
    desc.trim().to_string()
}

// ── Public API ────────────────────────────────────────────────────────────

/// Add all standard OpenCADStudio linetypes to `doc`, skipping existing ones.
pub fn populate_document(doc: &mut CadDocument) {
    for mut lt in parse(LIN_SOURCE) {
        if !doc.line_types.contains(&lt.name) {
            lt.set_handle(doc.allocate_handle());
            doc.line_types.add(lt).ok();
        }
    }
}


// ── Parser ────────────────────────────────────────────────────────────────

/// Parse a `.lin` file and return all simple linetypes found.
fn parse(src: &str) -> Vec<LineType> {
    let mut result = Vec::new();
    let mut current: Option<(String, String)> = None; // (name, description)

    for raw in src.lines() {
        let line = raw.trim();

        // Skip comments and blank lines.
        if line.is_empty() || line.starts_with(";;") {
            continue;
        }

        if let Some(rest) = line.strip_prefix('*') {
            // New pattern header: *NAME,Description
            if let Some((name, desc)) = rest.split_once(',') {
                current = Some((name.trim().to_string(), desc.trim().to_string()));
            }
        } else if line.to_ascii_uppercase().starts_with('A') {
            // Element line: A,v1,v2,...
            let Some((name, desc)) = current.take() else {
                continue;
            };

            let after_a = match line[1..].trim_start().strip_prefix(',') {
                Some(s) => s,
                None => continue,
            };

            let elements = parse_elements(after_a);
            let pattern_length: f64 = elements.iter().map(|e| e.length.abs()).sum();

            let mut lt = LineType::new(&name);
            lt.description = desc;
            lt.pattern_length = pattern_length;
            for e in elements {
                lt.add_element(e);
            }
            result.push(lt);
        }
    }

    result
}

/// Parse the comma-separated element values after `A,`.
/// Complex elements `[...]` are skipped; only plain numbers are used.
fn parse_elements(s: &str) -> Vec<LineTypeElement> {
    let mut elements = Vec::new();
    let mut chars = s.chars().peekable();
    let mut token = String::new();

    while let Some(&c) = chars.peek() {
        if c == '[' {
            // Skip entire complex element including nested brackets.
            let mut depth = 0usize;
            for ch in chars.by_ref() {
                if ch == '[' {
                    depth += 1;
                } else if ch == ']' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            // Consume trailing comma if present.
            if chars.peek() == Some(&',') {
                chars.next();
            }
            continue;
        }

        if c == ',' {
            chars.next();
            push_element(&token, &mut elements);
            token.clear();
        } else {
            token.push(c);
            chars.next();
        }
    }
    push_element(&token, &mut elements);

    elements
}

fn push_element(token: &str, out: &mut Vec<LineTypeElement>) {
    let t = token.trim();
    if t.is_empty() {
        return;
    }
    if let Ok(v) = t.parse::<f64>() {
        let elem = if v > 0.0 {
            LineTypeElement::dash(v)
        } else if v < 0.0 {
            LineTypeElement::space(v.abs())
        } else {
            LineTypeElement::dot()
        };
        out.push(elem);
    }
}

// ── Complex linetype parser ───────────────────────────────────────────────

/// Parse the LIN source and return a catalog of complex linetypes.
/// A linetype is "complex" when its A-line contains at least one `[SHAPE,...]`
/// element.  Simple (dash-only) linetypes are excluded.
fn parse_complex(src: &str) -> HashMap<String, ComplexLt> {
    let mut catalog: HashMap<String, ComplexLt> = HashMap::new();
    let mut current: Option<(String, String)> = None; // (name, description)

    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(";;") {
            continue;
        }

        if let Some(rest) = line.strip_prefix('*') {
            if let Some((name, desc)) = rest.split_once(',') {
                current = Some((name.trim().to_string(), desc.trim().to_string()));
            }
        } else if line.to_ascii_uppercase().starts_with('A') {
            let Some((name, _desc)) = current.take() else {
                continue;
            };

            let after_a = match line[1..].trim_start().strip_prefix(',') {
                Some(s) => s,
                None => continue,
            };

            // Only collect linetypes that contain a shape element.
            if !after_a.contains('[') {
                continue;
            }

            let segments = parse_complex_elements(after_a);
            if segments.is_empty() {
                continue;
            }

            catalog.insert(name.to_ascii_uppercase(), ComplexLt { segments });
        }
    }
    catalog
}

/// Parse complex A-line elements into `LtSegment`s.
/// Shape elements: `[SHAPENAME,fontfile,x=v,y=v,s=v,r=v]`
/// Text elements:  `["TEXT",font,...]` — skipped (returns no segment).
fn parse_complex_elements(s: &str) -> Vec<LtSegment> {
    let mut segs = Vec::new();
    let mut chars = s.chars().peekable();
    let mut token = String::new();

    while let Some(&c) = chars.peek() {
        if c == '[' {
            // Flush any pending numeric token.
            push_lt_segment(&token, &mut segs);
            token.clear();

            // Read the entire bracketed element.
            let mut depth = 0usize;
            let mut inner = String::new();
            for ch in chars.by_ref() {
                if ch == '[' {
                    depth += 1;
                    if depth > 1 {
                        inner.push(ch);
                    }
                } else if ch == ']' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    inner.push(ch);
                } else {
                    inner.push(ch);
                }
            }
            // Consume trailing comma.
            if chars.peek() == Some(&',') {
                chars.next();
            }

            if let Some(seg) = parse_shape_element(&inner) {
                segs.push(seg);
            }
        } else if c == ',' {
            chars.next();
            push_lt_segment(&token, &mut segs);
            token.clear();
        } else {
            token.push(c);
            chars.next();
        }
    }
    push_lt_segment(&token, &mut segs);
    segs
}

/// Parse a bracketed linetype element.
/// Handles both shape (`SHAPENAME,...`) and text (`"string",...`) elements.
fn parse_shape_element(inner: &str) -> Option<LtSegment> {
    let inner = inner.trim();
    if inner.starts_with('"') {
        return parse_text_element(inner);
    }

    let mut parts = inner.split(',');
    let name = parts.next()?.trim().to_string();
    if name.is_empty() {
        return None;
    }

    let mut x = 0.0f32;
    let mut y = 0.0f32;
    let mut scale = 1.0f32;
    let mut rot_deg = 0.0f32;

    for part in parts {
        let p = part.trim();
        if let Some(v) = p.strip_prefix("x=").or_else(|| p.strip_prefix("X=")) {
            x = v.parse().unwrap_or(0.0);
        } else if let Some(v) = p.strip_prefix("y=").or_else(|| p.strip_prefix("Y=")) {
            y = v.parse().unwrap_or(0.0);
        } else if let Some(v) = p.strip_prefix("s=").or_else(|| p.strip_prefix("S=")) {
            scale = v.parse().unwrap_or(1.0);
        } else if let Some(v) = p
            .strip_prefix("r=")
            .or_else(|| p.strip_prefix("R="))
            .or_else(|| p.strip_prefix("u="))
            .or_else(|| p.strip_prefix("U="))
        {
            rot_deg = v.parse().unwrap_or(0.0);
        }
    }

    Some(LtSegment::Shape {
        name,
        x,
        y,
        scale,
        rot_deg,
    })
}

/// Parse `"TEXT",STYLE,x=v,y=v,s=v,r=v` into an `LtSegment::Text`.
fn parse_text_element(inner: &str) -> Option<LtSegment> {
    // Find the closing quote.
    let rest = inner.strip_prefix('"')?;
    let end = rest.find('"')?;
    let text = rest[..end].to_string();
    let after = rest[end + 1..].trim().trim_start_matches(',');

    let mut parts = after.split(',');
    let style = parts
        .next()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let style = if style.is_empty() {
        "Standard".to_string()
    } else {
        style
    };

    let mut x = 0.0f32;
    let mut y = 0.0f32;
    let mut scale = 1.0f32;
    let mut rot_deg = 0.0f32;

    for part in parts {
        let p = part.trim();
        if let Some(v) = p.strip_prefix("x=").or_else(|| p.strip_prefix("X=")) {
            x = v.parse().unwrap_or(0.0);
        } else if let Some(v) = p.strip_prefix("y=").or_else(|| p.strip_prefix("Y=")) {
            y = v.parse().unwrap_or(0.0);
        } else if let Some(v) = p.strip_prefix("s=").or_else(|| p.strip_prefix("S=")) {
            scale = v.parse().unwrap_or(1.0);
        } else if let Some(v) = p
            .strip_prefix("r=")
            .or_else(|| p.strip_prefix("R="))
            .or_else(|| p.strip_prefix("u="))
            .or_else(|| p.strip_prefix("U="))
        {
            rot_deg = v.parse().unwrap_or(0.0);
        }
    }

    Some(LtSegment::Text {
        text,
        style,
        x,
        y,
        scale,
        rot_deg,
    })
}

fn push_lt_segment(token: &str, out: &mut Vec<LtSegment>) {
    let t = token.trim();
    if t.is_empty() {
        return;
    }
    if let Ok(v) = t.parse::<f32>() {
        let seg = if v > 0.0 {
            LtSegment::Dash(v)
        } else if v < 0.0 {
            LtSegment::Space(v.abs())
        } else {
            LtSegment::Dot
        };
        out.push(seg);
    }
}
