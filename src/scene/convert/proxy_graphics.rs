//! Proxy entity graphics — the cached vector preview an application stores
//! alongside a custom entity so viewers without its object enabler can still
//! draw something (e.g. an architecture door/wall, or an ACAD_TABLE, arrives
//! as an `Unknown` entity we cannot interpret, but it ships this preview).
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
//! Strings are NUL-terminated (8-bit or UTF-16LE) and padded to 4 bytes.
//!
//! Records decoded here:
//! * geometry — 2 circle (centre, radius, normal); 3 circle through 3 points;
//!   4 arc (centre, radius, normal, start vector, sweep, arc type); 5 arc
//!   through 3 points (+ arc type); 6 poly-line / 7 poly-gon (`u32 n`,
//!   `[f64;3]×n`); 9 shell (`u32 n` verts, then `u32` face-list length and
//!   `[i32 count, u32 idx…]` faces, a negative count marking a hole); 32 a
//!   poly-line followed by its normal; 33 an embedded LWPOLYLINE (`u32` byte
//!   count, then the entity's bit-coded body: flags, width, elevation,
//!   thickness, normal, points, bulges, vertex ids, widths).
//! * text — 10 / 36: position, normal, direction, height, width factor,
//!   oblique, then an 8-bit (10) or UTF-16 (36) string; 11 / 38: position,
//!   normal, direction, string, two longs, height, width factor, oblique,
//!   tracking, five style flags, (38 only: four font longs and a typeface),
//!   then the font file name.
//! * traits — 14 colour (ACI: 0 ByBlock, 256 ByLayer); 16 layer (index into
//!   the layer table); 18 linetype (index into the linetype table, 32767
//!   ByLayer, 32766 ByBlock); 20 fill (1 always, 2 never); 22 colour (encoded:
//!   0xC2 true colour / 0xC3 ACI / 0xC0 ByLayer / 0xC1 ByBlock); 23
//!   lineweight (0.01 mm); 29 push a 4×4 model transform (composed onto the
//!   current one); 31 pop it.

use glam::{DMat4, DVec3};

/// The colour a preview primitive draws in, from its colour traits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProxyColor {
    /// The colour of the primitive's layer (see [`ProxyStyle::layer`]).
    ByLayer,
    /// The proxy entity's own colour.
    ByBlock,
    /// A specific colour index (1..=255).
    Aci(u8),
    /// A specific true colour.
    Rgb(u8, u8, u8),
}

/// The sub-entity traits in force when a primitive was emitted. Until a
/// trait record says otherwise they are the proxy entity's own (ByBlock).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProxyStyle {
    pub color: ProxyColor,
    /// Index into the layer table; `None` = the proxy entity's own layer.
    pub layer: Option<u32>,
    /// Index into the linetype table (ByLayer / ByBlock entries excluded);
    /// [`LT_BYLAYER`] / [`LT_BYBLOCK`] inherit.
    pub linetype: u32,
    /// Lineweight, 0.01 mm units; negative = inherit the entity's.
    pub lineweight: i16,
}

pub const LT_BYLAYER: u32 = 32767;
pub const LT_BYBLOCK: u32 = 32766;

impl Default for ProxyStyle {
    fn default() -> Self {
        Self {
            color: ProxyColor::ByBlock,
            layer: None,
            linetype: LT_BYBLOCK,
            lineweight: -1,
        }
    }
}

/// A poly-line lifted from a proxy-graphics blob, in world coordinates. Arcs,
/// circles, closed poly-gons and shell faces are pre-flattened into points.
pub struct ProxyPolyline {
    pub points: Vec<[f64; 3]>,
    pub style: ProxyStyle,
}

/// A filled area (fill trait on), as world-space triangles (3 points each).
pub struct ProxyFill {
    pub tris: Vec<[f64; 3]>,
    pub style: ProxyStyle,
}

/// A single-line text label from a proxy-graphics blob, in world coordinates.
pub struct ProxyText {
    pub position: [f64; 3],
    pub height: f64,
    pub rotation: f64,
    pub width_factor: f64,
    pub oblique: f64,
    pub text: String,
    pub font: String,
    pub style: ProxyStyle,
}

/// Everything a preview decodes to.
#[derive(Default)]
pub struct Decoded {
    pub polylines: Vec<ProxyPolyline>,
    pub fills: Vec<ProxyFill>,
    pub texts: Vec<ProxyText>,
}

impl Decoded {
    pub fn is_empty(&self) -> bool {
        self.polylines.is_empty() && self.fills.is_empty() && self.texts.is_empty()
    }
}

const REC_CIRCLE: u32 = 2;
const REC_CIRCLE_3P: u32 = 3;
const REC_ARC: u32 = 4;
const REC_ARC_3P: u32 = 5;
const REC_POLYLINE: u32 = 6;
const REC_POLYGON: u32 = 7;
const REC_SHELL: u32 = 9;
const REC_TEXT: u32 = 10;
const REC_TEXT2: u32 = 11;
const REC_COLOR: u32 = 14;
const REC_LAYER: u32 = 16;
const REC_LINETYPE: u32 = 18;
const REC_FILL: u32 = 20;
const REC_COLOR_ENC: u32 = 22;
const REC_LINEWEIGHT: u32 = 23;
const REC_PUSH_MATRIX: u32 = 29;
const REC_PUSH_MATRIX2: u32 = 30;
const REC_POP_MATRIX: u32 = 31;
const REC_POLYLINE_N: u32 = 32;
const REC_LWPOLYLINE: u32 = 33;
const REC_TEXT_UNICODE: u32 = 36;
const REC_TEXT2_UNICODE: u32 = 38;

/// Fill trait value meaning "fill closed primitives".
const FILL_ALWAYS: u32 = 1;

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn f64_at(b: &[u8], o: usize) -> Option<f64> {
    let v = b
        .get(o..o + 8)
        .map(|s| f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))?;
    (v.is_finite() && v.abs() < 1e12).then_some(v)
}

fn pt3_at(b: &[u8], o: usize) -> Option<DVec3> {
    Some(DVec3::new(f64_at(b, o)?, f64_at(b, o + 8)?, f64_at(b, o + 16)?))
}

fn align4(o: usize) -> usize {
    (o + 3) & !3
}

/// A NUL-terminated, 4-byte-padded string at `data[o..]`; returns it and the
/// offset after the padding.
fn padded_string(data: &[u8], o: usize, unicode: bool) -> Option<(String, usize)> {
    let raw = data.get(o..)?;
    if unicode {
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        let end = o + units.len() * 2 + 2;
        Some((String::from_utf16_lossy(&units), align4(end)))
    } else {
        let n = raw.iter().position(|&b| b == 0)?;
        Some((String::from_utf8_lossy(&raw[..n]).into_owned(), align4(o + n + 1)))
    }
}

fn color_from_aci(v: u32) -> ProxyColor {
    match v {
        0 => ProxyColor::ByBlock,
        1..=255 => ProxyColor::Aci(v as u8),
        _ => ProxyColor::ByLayer,
    }
}

fn color_from_encoded(v: u32) -> ProxyColor {
    match v >> 24 {
        0xC2 => ProxyColor::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
        0xC1 => ProxyColor::ByBlock,
        0xC3 => color_from_aci(v & 0xFF),
        _ => ProxyColor::ByLayer,
    }
}

/// Decoder state: the traits and model transform in force.
struct State {
    out: Decoded,
    style: ProxyStyle,
    fill: bool,
    xform: DMat4,
    stack: Vec<DMat4>,
}

impl State {
    /// Transform a local run to world and emit it as one poly-line; a closed
    /// run under the fill trait is emitted as a filled area too.
    fn push(&mut self, local: Vec<DVec3>, closed: bool) {
        if local.len() < 2 {
            return;
        }
        let mut pts: Vec<[f64; 3]> = local
            .iter()
            .map(|&p| self.xform.transform_point3(p).to_array())
            .collect();
        if closed && self.fill && pts.len() >= 3 {
            let tris = crate::entities::mesh::triangulate_planar(&pts);
            if !tris.is_empty() {
                self.out.fills.push(ProxyFill {
                    tris,
                    style: self.style,
                });
            }
        }
        if closed {
            pts.push(pts[0]);
        }
        self.out.polylines.push(ProxyPolyline {
            points: pts,
            style: self.style,
        });
    }

    fn push_text(&mut self, mut t: ProxyText, dir: DVec3) {
        let dir = if dir.length_squared() > 0.0 {
            dir.normalize()
        } else {
            DVec3::X
        };
        let world_dir = self.xform.transform_vector3(dir);
        t.position = self.xform.transform_point3(DVec3::from(t.position)).to_array();
        t.height *= world_dir.length();
        t.rotation = world_dir.y.atan2(world_dir.x);
        t.style = self.style;
        if !t.text.trim().is_empty() && t.height.is_finite() && t.height > 0.0 {
            self.out.texts.push(t);
        }
    }
}

/// Decode every geometry / text record in a proxy-graphics `blob` into
/// world-space primitives. Returns an empty result when the blob is absent,
/// malformed, or carries nothing this decoder models — never invented shapes.
pub fn decode(blob: &[u8]) -> Decoded {
    let mut st = State {
        out: Decoded::default(),
        style: ProxyStyle::default(),
        fill: false,
        xform: DMat4::IDENTITY,
        stack: Vec::new(),
    };
    let total = match u32_at(blob, 0) {
        Some(t) => t as usize,
        None => return st.out,
    };
    if total > blob.len() || total < 8 {
        return st.out;
    }
    let count = u32_at(blob, 4).unwrap_or(0);
    if count > 1_000_000 {
        return st.out;
    }

    let mut pos = 8usize;
    for _ in 0..count {
        let (Some(rsize), Some(rtype)) = (u32_at(blob, pos), u32_at(blob, pos + 4)) else {
            break;
        };
        let rsize = rsize as usize;
        if rsize < 8 || pos + rsize > total {
            break;
        }
        let d = &blob[pos + 8..pos + rsize];
        let word = u32_at(d, 0);
        match rtype {
            REC_COLOR => {
                if let Some(c) = word {
                    st.style.color = color_from_aci(c);
                }
            }
            REC_COLOR_ENC => {
                if let Some(c) = word {
                    st.style.color = color_from_encoded(c);
                }
            }
            REC_LAYER => st.style.layer = word.or(st.style.layer),
            REC_LINETYPE => st.style.linetype = word.unwrap_or(st.style.linetype),
            REC_FILL => st.fill = word == Some(FILL_ALWAYS),
            REC_LINEWEIGHT => {
                if let Some(w) = word {
                    st.style.lineweight = w as i32 as i16;
                }
            }
            REC_PUSH_MATRIX => {
                // 4×4 row-major, composed onto the current model transform.
                let vals: Option<Vec<f64>> = (0..16).map(|k| f64_at(d, 8 * k)).collect();
                let m = vals.map_or(DMat4::IDENTITY, |v| {
                    let a: [f64; 16] = v.try_into().unwrap_or([0.0; 16]);
                    DMat4::from_cols_array(&a).transpose()
                });
                st.stack.push(st.xform);
                st.xform *= m;
            }
            // ponytail: the second push form's payload is undocumented and
            // unseen in samples; keep the stack balanced for its pop.
            REC_PUSH_MATRIX2 => st.stack.push(st.xform),
            REC_POP_MATRIX => st.xform = st.stack.pop().unwrap_or(DMat4::IDENTITY),
            REC_POLYLINE | REC_POLYGON | REC_POLYLINE_N => {
                if let Some(pts) = decode_points(d) {
                    st.push(pts, rtype == REC_POLYGON);
                }
            }
            REC_SHELL => {
                for face in decode_shell(d) {
                    st.push(face, true);
                }
            }
            REC_CIRCLE => {
                if let (Some(c), Some(r), Some(n)) = (pt3_at(d, 0), f64_at(d, 24), pt3_at(d, 32)) {
                    if let Some(pts) = arc_points(c, r, n, None, std::f64::consts::TAU) {
                        st.push(pts, true);
                    }
                }
            }
            REC_CIRCLE_3P => {
                if let Some((c, r, n, _, _)) = three_point_circle(d) {
                    if let Some(pts) = arc_points(c, r, n, None, std::f64::consts::TAU) {
                        st.push(pts, true);
                    }
                }
            }
            REC_ARC | REC_ARC_3P => {
                let arc = if rtype == REC_ARC {
                    (|| {
                        let (c, r, n) = (pt3_at(d, 0)?, f64_at(d, 24)?, pt3_at(d, 32)?);
                        Some((c, r, n, pt3_at(d, 56)?, f64_at(d, 80)?, u32_at(d, 88)))
                    })()
                } else {
                    three_point_circle(d).map(|(c, r, n, s, w)| (c, r, n, s, w, u32_at(d, 72)))
                };
                if let Some((c, r, n, start, sweep, kind)) = arc {
                    if let Some(mut pts) = arc_points(c, r, n, Some(start), sweep) {
                        // Arc type: 0 open arc, 1 sector (pie), 2 chord.
                        let closed = matches!(kind, Some(1) | Some(2));
                        if kind == Some(1) {
                            pts.push(c);
                        }
                        st.push(pts, closed);
                    }
                }
            }
            REC_LWPOLYLINE => {
                if let Some((pts, closed)) = decode_lwpolyline(d) {
                    st.push(pts, closed);
                }
            }
            REC_TEXT | REC_TEXT_UNICODE => {
                if let Some((t, dir)) = decode_text(d, rtype == REC_TEXT_UNICODE) {
                    st.push_text(t, dir);
                }
            }
            REC_TEXT2 | REC_TEXT2_UNICODE => {
                if let Some((t, dir)) = decode_text2(d, rtype == REC_TEXT2_UNICODE) {
                    st.push_text(t, dir);
                }
            }
            // Any other record is an unmodelled trait; `rsize` skips it.
            _ => {}
        }
        pos += rsize;
    }
    st.out
}

/// Read `[u32 n, [f64;3]×n]`.
fn decode_points(d: &[u8]) -> Option<Vec<DVec3>> {
    let n = u32_at(d, 0)? as usize;
    if n < 2 || 4 + n * 24 > d.len() {
        return None;
    }
    (0..n).map(|i| pt3_at(d, 4 + i * 24)).collect()
}

/// Read a shell record and return each face (holes included) as a vertex run.
fn decode_shell(d: &[u8]) -> Vec<Vec<DVec3>> {
    let mut faces = Vec::new();
    let Some(n) = u32_at(d, 0).map(|n| n as usize) else {
        return faces;
    };
    if n < 2 || 4 + n * 24 + 4 > d.len() {
        return faces;
    }
    let Some(verts) = (0..n).map(|i| pt3_at(d, 4 + i * 24)).collect::<Option<Vec<_>>>() else {
        return faces;
    };
    let mut fp = 4 + n * 24;
    let entries = u32_at(d, fp).unwrap_or(0) as usize;
    fp += 4;
    let mut read = 0;
    while read < entries {
        // A negative count marks a hole loop; it is drawn all the same.
        let Some(fc) = u32_at(d, fp).map(|c| (c as i32).unsigned_abs() as usize) else {
            break;
        };
        if fc > n || fp + 4 + fc * 4 > d.len() {
            break;
        }
        let face: Option<Vec<DVec3>> = (0..fc)
            .map(|k| u32_at(d, fp + 4 + k * 4).and_then(|i| verts.get(i as usize).copied()))
            .collect();
        match face {
            Some(face) => faces.push(face),
            None => break,
        }
        read += 1 + fc;
        fp += 4 + fc * 4;
    }
    faces
}

/// Centre, radius, normal, start vector and sweep of the circle through the
/// three points at `d` (sweep runs from the first point through the second
/// to the third).
fn three_point_circle(d: &[u8]) -> Option<(DVec3, f64, DVec3, DVec3, f64)> {
    let (p1, p2, p3) = (pt3_at(d, 0)?, pt3_at(d, 24)?, pt3_at(d, 48)?);
    let (a, b) = (p1 - p3, p2 - p3);
    let axb = a.cross(b);
    if axb.length_squared() < 1e-24 {
        return None;
    }
    let c = p3 + (b * a.length_squared() - a * b.length_squared()).cross(axb)
        / (2.0 * axb.length_squared());
    let n = (p2 - p1).cross(p3 - p2).normalize();
    let u = (p1 - c).normalize();
    let w = p3 - c;
    let mut sweep = w.dot(n.cross(u)).atan2(w.dot(u));
    if sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }
    Some((c, (p1 - c).length(), n, p1 - c, sweep))
}

/// Flatten an arc in the plane of `normal` (counter-clockwise about it) into
/// points; `start` is the direction of the first point (any in-plane
/// direction for a full circle).
fn arc_points(
    center: DVec3,
    radius: f64,
    normal: DVec3,
    start: Option<DVec3>,
    sweep: f64,
) -> Option<Vec<DVec3>> {
    if radius <= 0.0 || radius > 1e9 || !sweep.is_finite() || normal.length_squared() == 0.0 {
        return None;
    }
    let n = normal.normalize();
    let u = start
        .map(|s| s - n * s.dot(n))
        .filter(|s| s.length_squared() > 0.0)
        .unwrap_or_else(|| n.any_orthonormal_vector())
        .normalize();
    let v = n.cross(u);
    let closed = sweep.abs() >= std::f64::consts::TAU - 1e-9;
    let segs = ((sweep.abs() / std::f64::consts::TAU * 64.0).ceil() as usize).clamp(2, 512);
    // A full circle is closed by the caller; don't repeat its first point.
    let last = if closed { segs - 1 } else { segs };
    Some(
        (0..=last)
            .map(|i| {
                let a = sweep * (i as f64 / segs as f64);
                center + (u * a.cos() + v * a.sin()) * radius
            })
            .collect(),
    )
}

/// Decode an embedded LWPOLYLINE body into its (local) vertex run, arc
/// segments flattened, and whether it is closed. Widths are not drawn.
fn decode_lwpolyline(d: &[u8]) -> Option<(Vec<DVec3>, bool)> {
    use document::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
    use document::io::dwg::dwg_version::DwgVersion;
    let body = d.get(4..)?.to_vec();
    let max_items = body.len() * 4;
    let mut r = DwgBitReader::new(body, DwgVersion::AC24, document::types::DxfVersion::AC1032);
    let flags = r.read_bit_short() as u16;
    if flags & 4 != 0 {
        r.read_bit_double(); // constant width
    }
    let elevation = if flags & 8 != 0 { r.read_bit_double() } else { 0.0 };
    if flags & 2 != 0 {
        r.read_bit_double(); // thickness
    }
    let normal = if flags & 1 != 0 {
        let n = r.read_3bit_double();
        DVec3::new(n.x, n.y, n.z)
    } else {
        DVec3::Z
    };
    let count = |v: i32| (0..=max_items as i32).contains(&v).then_some(v as usize);
    let n_points = count(r.read_bit_long())?;
    let n_bulges = if flags & 16 != 0 { count(r.read_bit_long())? } else { 0 };
    let n_ids = if flags & 1024 != 0 { count(r.read_bit_long())? } else { 0 };
    if flags & 32 != 0 {
        count(r.read_bit_long())?; // widths follow the ids; unused
    }
    if n_points < 2 {
        return None;
    }
    let mut pts = vec![[r.read_raw_double(), r.read_raw_double()]];
    for i in 1..n_points {
        let [px, py] = pts[i - 1];
        pts.push([r.read_bit_double_with_default(px), r.read_bit_double_with_default(py)]);
    }
    let bulges: Vec<f64> = (0..n_bulges).map(|_| r.read_bit_double()).collect();
    for _ in 0..n_ids {
        r.read_bit_long();
    }
    if r.check_error().is_err() || pts.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let closed = flags & 512 != 0;
    let mut run: Vec<[f64; 2]> = Vec::with_capacity(n_points);
    for (i, &p) in pts.iter().enumerate() {
        run.push(p);
        let Some(&next) = pts.get(i + 1).or(closed.then_some(&pts[0])) else {
            break;
        };
        let bulge = bulges.get(i).copied().unwrap_or(0.0);
        if let Some(arc) = crate::entities::common::BulgeArc::from_bulge(p, next, bulge) {
            let samples = arc.tessellate_angle(std::f64::consts::PI / 32.0);
            run.extend_from_slice(&samples[1..samples.len() - 1]);
        }
    }
    let n = normal.normalize_or(DVec3::Z);
    let ((ax, ay, az), (bx, by, bz)) = crate::scene::view::transform::ocs_axes((n.x, n.y, n.z));
    let (ox, oy) = (DVec3::new(ax, ay, az), DVec3::new(bx, by, bz));
    let local = run
        .into_iter()
        .map(|[x, y]| ox * x + oy * y + n * elevation)
        .collect();
    Some((local, closed))
}

/// Decode a 10 / 36 text record: position, normal, direction, height, width
/// factor, oblique, then the padded string. Returns the label (local space)
/// and its direction.
fn decode_text(d: &[u8], unicode: bool) -> Option<(ProxyText, DVec3)> {
    let position = pt3_at(d, 0)?;
    let dir = pt3_at(d, 48)?;
    let (height, width_factor, oblique) = (f64_at(d, 72)?, f64_at(d, 80)?, f64_at(d, 88)?);
    let (text, _) = padded_string(d, 96, unicode)?;
    Some((
        ProxyText {
            position: position.to_array(),
            height,
            rotation: 0.0,
            width_factor,
            oblique,
            text,
            font: String::new(),
            style: ProxyStyle::default(),
        },
        dir,
    ))
}

/// Decode an 11 / 38 styled text record (see the module docs for the layout).
fn decode_text2(d: &[u8], unicode: bool) -> Option<(ProxyText, DVec3)> {
    let position = pt3_at(d, 0)?;
    let dir = pt3_at(d, 48)?;
    let (text, mut o) = padded_string(d, 72, unicode)?;
    o += 8; // string length, raw flag
    let (height, width_factor, oblique) = (f64_at(d, o)?, f64_at(d, o + 8)?, f64_at(d, o + 16)?);
    o += 32 + 20; // tracking, then backwards/upside-down/vertical/underline/overline
    if unicode {
        o += 16; // bold, italic, charset, pitch
        o = padded_string(d, o, true).map_or(o, |(_, next)| next); // typeface
    }
    let font = padded_string(d, o, unicode).map(|(f, _)| f).unwrap_or_default();
    Some((
        ProxyText {
            position: position.to_array(),
            height,
            rotation: 0.0,
            width_factor,
            oblique,
            text,
            font,
            style: ProxyStyle::default(),
        },
        dir,
    ))
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

    /// A placeholder preview's class-name label (unicode text record 36).
    #[test]
    fn decodes_the_placeholder_label() {
        let mut rec = Vec::new();
        for v in [2.0, 3.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.05, 1.0, 0.0] {
            rec.extend_from_slice(&f64::to_le_bytes(v));
        }
        for u in "AecDbWall".encode_utf16().chain([0, 0, 0]) {
            rec.extend_from_slice(&u.to_le_bytes());
        }
        let size = (8 + rec.len()) as u32;
        let mut blob = Vec::new();
        for v in [8 + size, 1, size, REC_TEXT_UNICODE] {
            blob.extend_from_slice(&v.to_le_bytes());
        }
        blob.extend_from_slice(&rec);
        let dec = decode(&blob);
        assert_eq!(dec.texts.len(), 1);
        assert_eq!(dec.texts[0].text, "AecDbWall");
        assert_eq!(dec.texts[0].position, [2.0, 3.0, 0.0]);
        assert!((dec.texts[0].height - 0.05).abs() < 1e-12);
    }

    /// Frame `(type, data)` records as a blob.
    fn blob(records: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let body: Vec<u8> = records
            .iter()
            .flat_map(|(t, d)| {
                let mut r = ((8 + d.len()) as u32).to_le_bytes().to_vec();
                r.extend_from_slice(&t.to_le_bytes());
                r.extend_from_slice(d);
                r
            })
            .collect();
        let mut b = ((8 + body.len()) as u32).to_le_bytes().to_vec();
        b.extend_from_slice(&(records.len() as u32).to_le_bytes());
        b.extend(body);
        b
    }

    fn f64s(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn line(a: [f64; 3], b: [f64; 3]) -> Vec<u8> {
        let mut d = 2u32.to_le_bytes().to_vec();
        d.extend(f64s(&a));
        d.extend(f64s(&b));
        d
    }

    fn translate(x: f64) -> Vec<u8> {
        f64s(&[1., 0., 0., x, 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.])
    }

    /// Pushes compose onto the current transform; a pop restores the outer one.
    #[test]
    fn matrix_stack_composes_and_pops() {
        let seg = line([0.0; 3], [1.0, 0.0, 0.0]);
        let dec = decode(&blob(&[
            (REC_PUSH_MATRIX, translate(10.0)),
            (REC_PUSH_MATRIX, translate(5.0)),
            (REC_POLYLINE, seg.clone()),
            (REC_POP_MATRIX, vec![]),
            (REC_POLYLINE, seg.clone()),
            (REC_POP_MATRIX, vec![]),
            (REC_POLYLINE, seg),
        ]));
        let starts: Vec<f64> = dec.polylines.iter().map(|p| p.points[0][0]).collect();
        assert_eq!(starts, [15.0, 10.0, 0.0]);
    }

    /// Colour / layer / linetype traits stick to the primitives that follow.
    #[test]
    fn sub_entity_traits_apply_to_following_primitives() {
        let seg = line([0.0; 3], [1.0, 0.0, 0.0]);
        let word = |v: u32| v.to_le_bytes().to_vec();
        let dec = decode(&blob(&[
            (REC_POLYLINE, seg.clone()),
            (REC_LAYER, word(8)),
            (REC_LINETYPE, word(LT_BYLAYER)),
            (REC_COLOR, word(256)),
            (REC_POLYLINE, seg.clone()),
            (REC_COLOR_ENC, word(0xC2FF_8000)),
            (REC_POLYLINE, seg),
        ]));
        let styles: Vec<ProxyStyle> = dec.polylines.iter().map(|p| p.style).collect();
        assert_eq!(styles[0], ProxyStyle::default());
        assert_eq!(styles[1].color, ProxyColor::ByLayer);
        assert_eq!((styles[1].layer, styles[1].linetype), (Some(8), LT_BYLAYER));
        assert_eq!(styles[2].color, ProxyColor::Rgb(255, 128, 0));
    }

    /// Circle, centre arc and 3-point arc land on the circle; a filled
    /// polygon also yields fill triangles.
    #[test]
    fn decodes_circles_arcs_and_fills() {
        let on_circle = |p: &ProxyPolyline, r: f64| {
            p.points.iter().all(|q| ((q[0] * q[0] + q[1] * q[1]).sqrt() - r).abs() < 1e-9)
        };
        let mut arc = f64s(&[0., 0., 0., 2., 0., 0., 1., 1., 0., 0., std::f64::consts::PI]);
        arc.extend(0u32.to_le_bytes());
        let mut arc3 = f64s(&[3., 0., 0., 0., 3., 0., -3., 0., 0.]);
        arc3.extend(0u32.to_le_bytes());
        let mut square = 4u32.to_le_bytes().to_vec();
        square.extend(f64s(&[0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.]));
        let dec = decode(&blob(&[
            (REC_CIRCLE, f64s(&[0., 0., 0., 1., 0., 0., 1.])),
            (REC_ARC, arc),
            (REC_ARC_3P, arc3),
            (REC_FILL, FILL_ALWAYS.to_le_bytes().to_vec()),
            (REC_POLYGON, square),
        ]));
        assert!(on_circle(&dec.polylines[0], 1.0));
        let first = dec.polylines[0].points[0];
        assert_eq!(dec.polylines[0].points.last(), Some(&first));
        let arc = &dec.polylines[1];
        assert!(on_circle(arc, 2.0));
        assert!((arc.points.last().unwrap()[0] + 2.0).abs() < 1e-9);
        assert!(arc.points.iter().all(|q| q[1] >= -1e-9));
        let arc3 = &dec.polylines[2];
        assert!(on_circle(arc3, 3.0));
        assert!((arc3.points.last().unwrap()[0] + 3.0).abs() < 1e-9);
        assert!(arc3.points.iter().all(|q| q[1] >= -1e-9));
        assert_eq!(dec.fills.len(), 1);
        assert_eq!(dec.fills[0].tris.len(), 6);
    }

    /// A styled unicode label: height comes from its own field, not the
    /// (unit) direction vector, and the font name follows the typeface.
    #[test]
    fn decodes_styled_unicode_text() {
        let utf16 = |s: &str| {
            let mut b: Vec<u8> = s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
            b.resize((b.len() + 3) & !3, 0);
            b
        };
        let mut d = f64s(&[5., 6., 0., 0., 0., 1., 1., 0., 0.]);
        d.extend(utf16("D1"));
        d.extend([0u8; 8]);
        d.extend(f64s(&[0.118, 0.9, 0.0, 1.0]));
        d.extend([0u8; 20 + 16]);
        d.extend(utf16("Arial Narrow"));
        d.extend(utf16("ARIALN.TTF"));
        d.extend(utf16(""));
        let dec = decode(&blob(&[(REC_TEXT2_UNICODE, d)]));
        let t = &dec.texts[0];
        assert_eq!((t.text.as_str(), t.font.as_str()), ("D1", "ARIALN.TTF"));
        assert_eq!(t.position, [5.0, 6.0, 0.0]);
        assert!((t.height - 0.118).abs() < 1e-12 && (t.width_factor - 0.9).abs() < 1e-12);
    }

    /// An embedded LWPOLYLINE: closed, one bulged (semicircle) segment.
    #[test]
    fn decodes_embedded_lwpolyline_with_bulge() {
        use document::io::dwg::dwg_stream_writers::bit_writer::DwgBitWriter;
        use document::io::dwg::dwg_version::DwgVersion;
        let mut w = DwgBitWriter::new(DwgVersion::AC24, document::types::DxfVersion::AC1032);
        w.write_bit_short(512 | 16 | 8); // closed, has bulges, elevation
        w.write_bit_double(2.0);
        w.write_bit_long(2);
        w.write_bit_long(2);
        w.write_raw_double(0.0);
        w.write_raw_double(0.0);
        // (value, default): the default is the previous vertex.
        w.write_bit_double_with_default(4.0, 0.0);
        w.write_bit_double_with_default(0.0, 0.0);
        w.write_bit_double(1.0);
        w.write_bit_double(0.0);
        let body = w.into_bytes();
        let mut d = (body.len() as u32).to_le_bytes().to_vec();
        d.extend(body);
        let dec = decode(&blob(&[(REC_LWPOLYLINE, d)]));
        let pts = &dec.polylines[0].points;
        assert!(pts.len() > 4 && pts.iter().all(|p| p[2] == 2.0));
        assert_eq!(pts.first(), pts.last());
        // Bulge 1 from (0,0) to (4,0): a semicircle of radius 2 about (2,0).
        assert!(pts.iter().any(|p| (p[1].abs() - 2.0).abs() < 1e-9));
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
