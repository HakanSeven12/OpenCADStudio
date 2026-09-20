//! Proxy entity graphics — the cached vector preview an application stores
//! alongside a custom entity so viewers without its object enabler can still
//! draw something. AutoCAD falls back to exactly this; so do we (e.g. an
//! AutoCAD Architecture door/wall, or an ACAD_TABLE, arrives as an `Unknown`
//! entity we cannot interpret, but it ships this preview).
//!
//! LibreDWG and ACadSharp both keep the blob raw and never parse it, so there
//! is no reference decoder to copy. The layout below was reverse-engineered
//! from real previews (a Raster Design image frame; an ACA door, wall; an
//! ACAD_TABLE) and is deliberately conservative: it reads only the primitive
//! records it has verified and treats every other record as an opaque trait.
//!
//! Blob grammar (all little-endian):
//! ```text
//! u32 total_size        (== blob length)
//! u32 record_count
//! record_count × {
//!     u32 record_size   (bytes, including this 8-byte header)
//!     u32 record_type
//!     u8[record_size-8] data
//! }
//! ```
//! Record types decoded here:
//! * geometry — 6 poly-line / 7 poly-gon (closed): `u32 n`, then `[f64;3]×n`;
//!   4/5 circular arc (centre, radius, normal, start dir, sweep); 9 shell
//!   (`u32 n` verts, then a face list of `[u32 count, u32 idx…]`); 32 a
//!   normal-tagged poly-line (`u32 n`, `[f64;3]×n`, then a normal).
//! * text — 38: position, normal, direction (its length is the glyph height),
//!   then a UTF-16 content string and a UTF-16 font (`*.shx`).
//! * traits — 14 colour (plain ACI); 22 colour (encoded: 0xC2 RGB / 0xC3 ACI);
//!   23 lineweight (0.01 mm); 29 a 4×4 transform whose local→world matrix
//!   applies to every following primitive (identity by default).

/// The colour a preview primitive draws in, from its colour traits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProxyColor {
    /// ByLayer / ByBlock — inherit the entity's own colour.
    Inherit,
    /// A specific AutoCAD Color Index (1..=255).
    Aci(u8),
    /// A specific true colour.
    Rgb(u8, u8, u8),
}

/// A poly-line lifted from a proxy-graphics blob, in world coordinates. Arcs,
/// closed poly-gons and shell faces are pre-flattened into points.
pub struct ProxyPolyline {
    pub points: Vec<[f64; 3]>,
    /// Colour in force when this primitive was emitted.
    pub color: ProxyColor,
    /// Lineweight in force, 0.01 mm units; negative = inherit the entity's.
    pub lineweight: i16,
}

/// A single-line text label from a proxy-graphics blob, in world coordinates.
pub struct ProxyText {
    pub position: [f64; 3],
    pub height: f64,
    pub rotation: f64,
    pub text: String,
    pub font: String,
    pub color: ProxyColor,
}

/// Everything a preview decodes to.
#[derive(Default)]
pub struct Decoded {
    pub polylines: Vec<ProxyPolyline>,
    pub texts: Vec<ProxyText>,
}

const REC_ARC: u32 = 4;
const REC_ARC5: u32 = 5;
const REC_POLYLINE: u32 = 6;
const REC_POLYGON: u32 = 7;
const REC_SHELL: u32 = 9;
/// Trait: current colour as a plain ACI (256 ByLayer / 0 ByBlock / 1..=255).
const REC_COLOR: u32 = 14;
/// Trait: current colour in encoded form (0xC2 true colour / 0xC3 indexed).
const REC_COLOR_ENC: u32 = 22;
/// Trait: current lineweight (0.01 mm; negative = ByLayer/ByBlock/Default).
const REC_LINEWEIGHT: u32 = 23;
/// Trait: a 4×4 local→world transform for the following primitives.
const REC_TRANSFORM: u32 = 29;
/// A normal-tagged poly-line (points then a trailing normal vector).
const REC_LINE_N: u32 = 32;
/// A text label (position/normal/direction, content string, font).
const REC_TEXT: u32 = 38;

/// Row-major 3×4 local→world transform (the bottom row of the 4×4 is 0,0,0,1).
type Xform = [f64; 12];
const IDENTITY: Xform = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

fn apply(m: &Xform, p: [f64; 3]) -> [f64; 3] {
    [
        m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[3],
        m[4] * p[0] + m[5] * p[1] + m[6] * p[2] + m[7],
        m[8] * p[0] + m[9] * p[1] + m[10] * p[2] + m[11],
    ]
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn f64_at(b: &[u8], o: usize) -> Option<f64> {
    b.get(o..o + 8)
        .map(|s| f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn pt3_at(b: &[u8], o: usize) -> Option<[f64; 3]> {
    let p = [f64_at(b, o)?, f64_at(b, o + 8)?, f64_at(b, o + 16)?];
    p.iter()
        .all(|v| v.is_finite() && v.abs() < 1e12)
        .then_some(p)
}

fn color_from_aci(v: i32) -> ProxyColor {
    match v {
        1..=255 => ProxyColor::Aci(v as u8),
        _ => ProxyColor::Inherit,
    }
}

fn color_from_encoded(v: u32) -> ProxyColor {
    match (v >> 24) & 0xFF {
        0xC2 => ProxyColor::Rgb(
            ((v >> 16) & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            (v & 0xFF) as u8,
        ),
        _ => color_from_aci((v & 0x00FF_FFFF) as i32),
    }
}

/// Decode every geometry / text record in a proxy-graphics `blob` into
/// world-space primitives. Returns an empty result when the blob is absent,
/// malformed, or carries nothing this decoder models — never invented shapes.
pub fn decode(blob: &[u8]) -> Decoded {
    let mut out = Decoded::default();
    let total = match u32_at(blob, 0) {
        Some(t) => t as usize,
        None => return out,
    };
    if total > blob.len() || total < 8 {
        return out;
    }
    let count = u32_at(blob, 4).unwrap_or(0);
    if count > 1_000_000 {
        return out;
    }

    let mut pos = 8usize;
    let mut color = ProxyColor::Inherit;
    let mut lineweight: i16 = -1;
    let mut xform = IDENTITY;

    for _ in 0..count {
        let Some(rsize) = u32_at(blob, pos) else {
            break;
        };
        let rsize = rsize as usize;
        let Some(rtype) = u32_at(blob, pos + 4) else {
            break;
        };
        if rsize < 8 || pos + rsize > total {
            break;
        }
        match rtype {
            REC_COLOR => {
                if let Some(c) = u32_at(blob, pos + 8) {
                    color = color_from_aci(c as i32);
                }
            }
            REC_COLOR_ENC => {
                if let Some(c) = u32_at(blob, pos + 8) {
                    color = color_from_encoded(c);
                }
            }
            REC_LINEWEIGHT => {
                if let Some(w) = u32_at(blob, pos + 8) {
                    lineweight = w as i32 as i16;
                }
            }
            REC_TRANSFORM => {
                // 4×4 row-major; keep the top 3 rows (local→world).
                let mut m = IDENTITY;
                if (0..12).all(|k| f64_at(blob, pos + 8 + 8 * k).map(|v| m[k] = v).is_some()) {
                    xform = m;
                }
            }
            REC_POLYLINE | REC_POLYGON | REC_LINE_N => {
                if let Some(pts) = decode_points(blob, pos, rsize, rtype == REC_POLYGON) {
                    push_line(&mut out, pts, color, lineweight, &xform);
                }
            }
            REC_SHELL => {
                for face in decode_shell(blob, pos, rsize) {
                    push_line(&mut out, face, color, lineweight, &xform);
                }
            }
            REC_ARC | REC_ARC5 => {
                if let Some(pts) = decode_arc(blob, pos) {
                    push_line(&mut out, pts, color, lineweight, &xform);
                }
            }
            REC_TEXT => {
                if let Some(mut t) = decode_text(blob, pos, rsize) {
                    t.position = apply(&xform, t.position);
                    t.color = color;
                    out.texts.push(t);
                }
            }
            // Any other record is an unmodelled trait; `rsize` skips it.
            _ => {}
        }
        pos += rsize;
    }
    out
}

/// Transform a run of local points to world and push it as one poly-line.
fn push_line(
    out: &mut Decoded,
    local: Vec<[f64; 3]>,
    color: ProxyColor,
    lineweight: i16,
    xform: &Xform,
) {
    if local.len() >= 2 {
        out.polylines.push(ProxyPolyline {
            points: local.iter().map(|&p| apply(xform, p)).collect(),
            color,
            lineweight,
        });
    }
}

/// Read `[u32 n, [f64;3]×n]` at a record; close it if `closed`.
fn decode_points(blob: &[u8], pos: usize, rsize: usize, closed: bool) -> Option<Vec<[f64; 3]>> {
    let n = u32_at(blob, pos + 8)? as usize;
    if n < 2 || 12 + n * 24 > rsize {
        return None;
    }
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..n {
        pts.push(pt3_at(blob, pos + 12 + i * 24)?);
    }
    if closed {
        if let Some(&first) = pts.first() {
            pts.push(first);
        }
    }
    Some(pts)
}

/// Read a shell record — `n` verts then a face list of `[count, idx…]` — and
/// return each face as a closed boundary poly-line.
fn decode_shell(blob: &[u8], pos: usize, rsize: usize) -> Vec<Vec<[f64; 3]>> {
    let mut faces = Vec::new();
    let Some(n) = u32_at(blob, pos + 8) else {
        return faces;
    };
    let n = n as usize;
    let vbase = pos + 12;
    if n < 2 || vbase + n * 24 > pos + rsize {
        return faces;
    }
    let verts: Vec<[f64; 3]> = match (0..n).map(|i| pt3_at(blob, vbase + i * 24)).collect() {
        Some(v) => v,
        None => return faces,
    };
    // Face list: [list_len, then (count, idx×count)…]. Walk indices, tolerating
    // the trailing edge/visibility data by stopping at the first bad count.
    let mut fp = vbase + n * 24;
    let end = pos + rsize;
    let _list_len = u32_at(blob, fp); // total face-list longs (unused)
    fp += 4;
    while fp + 4 <= end {
        let Some(fc) = u32_at(blob, fp) else { break };
        let fc = fc as usize;
        if fc < 2 || fc > n || fp + 4 + fc * 4 > end {
            break;
        }
        let mut loop_pts = Vec::with_capacity(fc + 1);
        let mut ok = true;
        for k in 0..fc {
            match u32_at(blob, fp + 4 + k * 4) {
                Some(idx) if (idx as usize) < n => loop_pts.push(verts[idx as usize]),
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            break;
        }
        if let Some(&first) = loop_pts.first() {
            loop_pts.push(first);
        }
        faces.push(loop_pts);
        fp += 4 + fc * 4;
    }
    faces
}

/// Flatten a circular-arc record into a strip of points (local coords).
fn decode_arc(blob: &[u8], pos: usize) -> Option<Vec<[f64; 3]>> {
    let center = pt3_at(blob, pos + 8)?;
    let radius = f64_at(blob, pos + 32)?;
    let normal = pt3_at(blob, pos + 40)?;
    let start_dir = pt3_at(blob, pos + 64)?;
    let sweep = f64_at(blob, pos + 88)?;
    if !radius.is_finite() || radius <= 0.0 || radius > 1e9 || !sweep.is_finite() {
        return None;
    }
    let start = start_dir[1].atan2(start_dir[0]);
    let dir = if normal[2] < 0.0 { -1.0 } else { 1.0 };
    let segs = ((sweep.abs() / std::f64::consts::TAU * 64.0).ceil() as usize).clamp(2, 512);
    let mut points = Vec::with_capacity(segs + 1);
    for i in 0..=segs {
        let a = start + dir * sweep * (i as f64 / segs as f64);
        points.push([
            center[0] + radius * a.cos(),
            center[1] + radius * a.sin(),
            center[2],
        ]);
    }
    Some(points)
}

/// Decode a text record: position + direction (its length is the height) + a
/// content string and font, all in local space (caller applies the transform).
fn decode_text(blob: &[u8], pos: usize, rsize: usize) -> Option<ProxyText> {
    let position = pt3_at(blob, pos + 8)?;
    // doubles: pos(3), normal(3), direction(3) — direction length = height.
    let dir = pt3_at(blob, pos + 8 + 48)?;
    let height = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    let rotation = dir[1].atan2(dir[0]);
    // UTF-16LE strings live at the record's tail: the content, then the font.
    let data = blob.get(pos + 8..pos + rsize)?;
    let mut strings = utf16_strings(data);
    let font = strings
        .iter()
        .position(|s| s.to_ascii_lowercase().ends_with(".shx"))
        .map(|i| strings.remove(i))
        .unwrap_or_default();
    let text = strings.into_iter().next().unwrap_or_default();
    if text.trim().is_empty() || !height.is_finite() || height <= 0.0 {
        return None;
    }
    Some(ProxyText {
        position,
        height,
        rotation,
        text,
        font,
        color: ProxyColor::Inherit,
    })
}

/// Pull the printable-ASCII UTF-16LE runs (≥ 2 chars) out of a byte slice.
fn utf16_strings(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i + 1 < data.len() {
        let (lo, hi) = (data[i], data[i + 1]);
        if hi == 0 && (0x20..0x7f).contains(&lo) {
            cur.push(lo as char);
        } else {
            if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
        i += 2;
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real 140-byte preview of an Autodesk Raster Design embedded raster
    /// image: a single type-6 poly-line, its frame closed back to the origin.
    #[test]
    fn decodes_the_raster_design_image_frame() {
        let hex = "8C00000001000000840000000600000005000000\
                   000000000000000000000000000000000000000000000000\
                   DEF97E6A1C422E3D3DDF4F8D976E8B400000000000000000\
                   6BBC749398A092403DDF4F8D976E8B400000000000000000\
                   6BBC7493 98A09240 0000000000000000 0000000000000000\
                   000000000000000000000000000000000000000000000000";
        let blob: Vec<u8> = hex
            .split_whitespace()
            .collect::<String>()
            .as_bytes()
            .chunks(2)
            .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(blob.len(), 140);
        let dec = decode(&blob);
        assert_eq!(dec.polylines.len(), 1);
        assert_eq!(dec.polylines[0].points.len(), 5);
        assert!((dec.polylines[0].points[2][0] - 1192.1490).abs() < 1e-3);
        assert!((dec.polylines[0].points[2][1] - 877.8240).abs() < 1e-3);
    }

    #[test]
    fn rejects_anything_it_does_not_model() {
        assert!(decode(&[]).polylines.is_empty());
        assert!(decode(&[0u8; 140]).polylines.is_empty());
        let mut b = vec![0u8; 140];
        b[0] = 140;
        assert!(decode(&b).polylines.is_empty());
    }
}
