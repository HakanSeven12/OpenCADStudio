//! DGN V7 (ISFF) design files: one model ("Default") of variable-length
//! element records. Coordinates are UORs; the design file header (TCB)
//! gives the global origin and the master unit the model is read in. Shared
//! cells are drawn where their instances place them.

use std::collections::HashMap;

use super::model::{
    arc_cubics, bspline_points, dgn_color_table, paths_bounds, scaled_elements, By, DgnCollector,
    DgnColor, DgnElement, DgnGeom, DgnImport, Path, PathBuilder, Segment, Sheet, SubPath, Text,
    DGN_DEFAULT_COLORS,
};

/// A 32-bit integer stored as two little-endian words, high word first.
fn int32(b: &[u8], at: usize) -> Option<i32> {
    let p = b.get(at..at + 4)?;
    Some(i32::from_le_bytes([p[2], p[3], p[0], p[1]]))
}

fn le_i32(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn uint16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

/// A VAX D-floating value as an IEEE double.
fn vax_double(b: &[u8], at: usize) -> Option<f64> {
    let s = b.get(at..at + 8)?;
    let hi = u32::from_le_bytes([s[2], s[3], s[0], s[1]]);
    let lo = u32::from_le_bytes([s[6], s[7], s[4], s[5]]);
    let exponent = ((hi >> 23) & 0xff) as i64;
    if exponent == 0 {
        return Some(0.0);
    }
    let exponent = (exponent - 129 + 1023) as u32;
    let rnd = lo & 7;
    let mut lo2 = (lo >> 3) | (hi << 29);
    if rnd != 0 {
        lo2 |= 1;
    }
    let hi2 = ((hi >> 3) & 0x000f_ffff) | (exponent << 20) | (hi & 0x8000_0000);
    Some(f64::from_bits(((hi2 as u64) << 32) | lo2 as u64))
}

/// The file is a V7 design file: it opens with a design file header.
pub fn is_v7(bytes: &[u8]) -> bool {
    bytes.len() > 1256 && (bytes[1] & 0x7f) == 9 && (bytes[0] & 0x3f) == 8
}

/// A 3D design file: the header element's 3D flag. Its points have a z,
/// and planar elements carry their plane as a quaternion.
fn is_3d(bytes: &[u8]) -> bool {
    bytes[0] & 0x40 != 0
}

/// The rows of the rotation a 3D element's quaternion (w, x, y, z as
/// signed 1.31 fractions) stands for: the element's own x, y and z axes.
fn quaternion_axes(e: &[u8], at: usize) -> Option<[[f64; 3]; 3]> {
    let q = |k: usize| int32(e, at + 4 * k).map(|v| v as f64 / 2_147_483_648.0);
    let (w, x, y, z) = (q(0)?, q(1)?, q(2)?, q(3)?);
    // The rotation matrix's columns are the axes.
    let m = [
        [
            x * x - y * y - z * z + w * w,
            2.0 * (z * w + x * y),
            2.0 * (x * z - y * w),
        ],
        [
            2.0 * (x * y - z * w),
            -x * x + y * y - z * z + w * w,
            2.0 * (x * w + y * z),
        ],
        [
            2.0 * (x * z + y * w),
            2.0 * (y * z - x * w),
            -x * x - y * y + z * z + w * w,
        ],
    ];
    Some([0, 1, 2].map(|c| [m[0][c], m[1][c], m[2][c]]))
}

/// A cell name in Radix-50: two words of three characters each.
fn radix50(e: &[u8], at: usize) -> String {
    const SET: &[u8] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZ$.?0123456789";
    let mut name = String::new();
    for k in 0..2 {
        let Some(mut word) = uint16(e, at + 2 * k).map(usize::from) else {
            break;
        };
        let mut chars = [0u8; 3];
        for c in chars.iter_mut().rev() {
            *c = SET.get(word % 40).copied().unwrap_or(b' ');
            word /= 40;
        }
        name.extend(chars.iter().map(|c| char::from(*c)));
    }
    name.trim().to_string()
}

struct Units {
    uor_per_sub: f64,
    sub_per_master: f64,
    origin: [f64; 2],
}

fn units(b: &[u8]) -> Option<Units> {
    let sub_per_master = int32(b, 1112)? as f64;
    let uor_per_sub = int32(b, 1116)? as f64;
    let origin = [vax_double(b, 1240)?, vax_double(b, 1248)?];
    (sub_per_master > 0.0 && uor_per_sub > 0.0).then_some(Units {
        uor_per_sub,
        sub_per_master,
        origin,
    })
}

/// The element records, in file order.
fn elements(b: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 4 <= b.len() && !(b[at] == 0xff && b[at + 1] == 0xff) {
        let Some(words) = uint16(b, at + 2) else {
            break;
        };
        let len = 4 + words as usize * 2;
        let Some(e) = b.get(at..at + len) else { break };
        out.push(e);
        at += len;
    }
    out
}

/// The file's colour table (element type 5 on level 1): index i < 255 at
/// 41 + 3i, the background (255) at 38.
fn color_table(elements: &[&[u8]]) -> Option<Vec<[u8; 3]>> {
    // A file with more than one colour table uses the last.
    elements
        .iter()
        .rev()
        .find(|e| e[1] == 5 && e[0] & 0x3f == 1)
        .and_then(|e| dgn_color_table(e, 41))
}

/// Model names of a V7 file (it has one).
pub fn model_names(bytes: &[u8]) -> Option<Vec<String>> {
    is_v7(bytes).then(|| vec!["Default".to_string()])
}

/// Fill colour index of an element's fill linkage (id 0x41), when it has one.
fn fill_index(e: &[u8]) -> Option<u8> {
    // The linkages follow the attribute index's word; the first one starts
    // at the first word flagged as user data.
    let base = 32 + uint16(e, 30)? as usize * 2;
    let mut at = (base..base + 16).step_by(2).find(|&at| {
        uint16(e, at).is_some_and(|h| h & 0x1000 != 0 && (1..=64).contains(&(h & 0xff)))
    })?;
    while at + 4 <= e.len() {
        let header = uint16(e, at)?;
        let words = (header & 0x00ff) as usize;
        if header & 0x1000 == 0 || words == 0 {
            break;
        }
        if uint16(e, at + 2)? == 0x41 {
            return e.get(at + 8).copied();
        }
        at += (words + 1) * 2;
    }
    None
}

/// Geometry in raw UORs, mapped to sheet units when placed.
#[derive(Default, Clone)]
struct Raw {
    paths: Vec<Path>,
    texts: Vec<Text>,
    instances: Vec<Instance>,
    /// The same run element by element, for DGNIMPORT.
    elements: Vec<DgnElement>,
}

#[derive(Clone)]
struct Instance {
    name: String,
    /// x' = m[0]·x + m[1]·y + origin.x, y' = m[2]·x + m[3]·y + origin.y.
    m: [f64; 4],
    origin: [f64; 2],
}

/// How deep cells inside cells are read.
const MAX_CELL_DEPTH: usize = 16;

/// The fractions of a UOR an element's coordinates carry past their whole
/// values: the user linkage 0x51A9 (after the attribute index at 30) holds a
/// count, a key word and that many signed 1/32768ths, one per coordinate in
/// the element's point order.
fn uor_fractions(e: &[u8]) -> Vec<f64> {
    let mut at = 32 + 2 * uint16(e, 30).unwrap_or(0) as usize;
    while let Some(h) = uint16(e, at) {
        if h & 0x1000 == 0 || h == 0 {
            break;
        }
        let words = (h & 0xff) as usize + 1;
        if uint16(e, at + 2) == Some(0x51a9) {
            let count = uint16(e, at + 4).unwrap_or(0) as usize;
            if 4 + count <= words {
                return (0..count)
                    .filter_map(|k| e.get(at + 8 + 2 * k..at + 10 + 2 * k))
                    .map(|w| i16::from_le_bytes([w[0], w[1]]) as f64 / 32768.0)
                    .collect();
            }
        }
        at += 2 * words;
    }
    Vec::new()
}

/// Single-byte text as code page 1252 reads it (a code it leaves
/// undefined stands for itself).
fn windows_1252(bytes: &[u8]) -> String {
    const HIGH: [u32; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013,
        0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => char::from_u32(HIGH[(b - 0x80) as usize]).unwrap_or('\u{FFFD}'),
            _ => char::from(b),
        })
        .collect()
}

/// A V7 file's single-byte text in a requested code page. V7 names no page,
/// so `None` keeps the historical 1252 behaviour; known pages decode through
/// `encoding_rs`, and unknown values fall back the same way rather than
/// promising mappings they cannot honour.
fn single_byte_text(bytes: &[u8], code_page: Option<u16>) -> String {
    match code_page {
        Some(page) => match code_page_encoding(page) {
            Some(encoding) => {
                let (text, _had_replacements) = encoding.decode_with_bom_removal(bytes);
                text.into_owned()
            }
            None => windows_1252(bytes),
        },
        None => windows_1252(bytes),
    }
}

/// The single-byte Windows code pages a V7 international MicroStation can
/// have written. The decoder only lists what `encoding_rs` names; a chosen
/// page outside this list keeps 1252.
pub(super) fn code_page_encoding(code_page: u16) -> Option<&'static encoding_rs::Encoding> {
    match code_page {
        1250 => Some(encoding_rs::WINDOWS_1250),
        1251 => Some(encoding_rs::WINDOWS_1251),
        1253 => Some(encoding_rs::WINDOWS_1253),
        1254 => Some(encoding_rs::WINDOWS_1254),
        1255 => Some(encoding_rs::WINDOWS_1255),
        1256 => Some(encoding_rs::WINDOWS_1256),
        1257 => Some(encoding_rs::WINDOWS_1257),
        1258 => Some(encoding_rs::WINDOWS_1258),
        874 => Some(encoding_rs::WINDOWS_874),
        932 => Some(encoding_rs::SHIFT_JIS),
        936 => Some(encoding_rs::GBK),
        949 => Some(encoding_rs::EUC_KR),
        950 => Some(encoding_rs::BIG5),
        65001 => Some(encoding_rs::UTF_8),
        _ => None,
    }
}

/// Reads a run of elements into raw geometry: plain elements, complex
/// chains and shapes (joined), B-splines, text, cells and shared cell
/// instances. In a 3D file (`d3`) points have a z and planar elements their
/// plane; the drawn geometry is their plan.
fn read_run(
    run: &[&[u8]],
    colors: &[[u8; 3]],
    d3: bool,
    depth: usize,
    code_page: Option<u16>,
) -> Raw {
    let mut raw = Raw::default();
    // An open B-spline curve: order, pole count, colour, closed, and the
    // header as the element it imports as.
    let mut spline: Option<(usize, usize, [u8; 3], bool, DgnElement)> = None;
    let mut knots: Vec<f64> = Vec::new();
    // A complex shape or chain collects its components into one path.
    let mut complex: Option<(
        usize,
        PathBuilder,
        Option<([u8; 3], f64)>,
        Option<[u8; 3]>,
        bool,
    )> = None;
    let push = |raw: &mut Raw,
                complex: &mut Option<(
        usize,
        PathBuilder,
        Option<([u8; 3], f64)>,
        Option<[u8; 3]>,
        bool,
    )>,
                path: Path| {
        if let Some((left, pb, _, _, _)) = complex.as_mut() {
            for sp in &path.subpaths {
                for seg in &sp.segments {
                    match seg {
                        Segment::Line(a, b) => {
                            if pb.current() != Some(*a) {
                                if pb.current().is_none() {
                                    pb.move_to(*a)
                                } else {
                                    pb.line_to(*a)
                                }
                            }
                            pb.line_to(*b);
                        }
                        Segment::Cubic(a, c1, c2, b) => {
                            if pb.current() != Some(*a) {
                                if pb.current().is_none() {
                                    pb.move_to(*a)
                                } else {
                                    pb.line_to(*a)
                                }
                            }
                            pb.cubic_to(*c1, *c2, *b);
                        }
                    }
                }
            }
            *left = left.saturating_sub(1);
            if *left == 0 {
                let (_, mut pb, stroke, fill, closed) = complex.take().unwrap();
                if closed {
                    pb.close();
                }
                raw.paths.push(Path {
                    subpaths: pb.finish(),
                    stroke,
                    fill,
                });
            }
        } else {
            raw.paths.push(path);
        }
    };
    let mut collect = DgnCollector::default();
    let mut index = 0;
    while index < run.len() {
        let e = run[index];
        index += 1;
        let kind = e[1] & 0x7f;
        if e[1] & 0x80 != 0 || e.len() < 36 {
            continue;
        }
        // An element that is no component ends the complex elements still
        // open.
        if e[0] & 0x80 == 0 {
            collect.close();
        }
        let symbology = uint16(e, 34).unwrap_or(0);
        let color = colors[(symbology >> 8) as usize];
        let element = |geom: DgnGeom| DgnElement {
            level: level(e) as u32,
            color: By::Own(DgnColor {
                index: (symbology >> 8) as u32,
                table: color,
                rgb: color,
            }),
            weight: By::Own(((symbology >> 3) & 31) as u32),
            style: By::Own((symbology & 7) as u32),
            geom,
            filled: fill_index(e).is_some(),
            construction: uint16(e, 32).unwrap_or(0) & 0xf == 2,
        };
        // One pixel wide, as V8 models draw: the element weight is a
        // lineweight, not shown while lineweights are off.
        let stroke = Some((color, -1.0));
        let fill = fill_index(e).map(|i| colors[i as usize]);
        // A point as the file holds it: two whole UORs, or three in 3D.
        let pt3 = |i: usize| -> Option<[f64; 3]> {
            Some([
                int32(e, i)? as f64,
                int32(e, i + 4)? as f64,
                if d3 { int32(e, i + 8)? as f64 } else { 0.0 },
            ])
        };
        let plan = |p: [f64; 3]| [p[0], p[1]];
        let step = if d3 { 12 } else { 8 };
        // Point `k` of the element (at `at`) with its UOR fractions, when the
        // element carries one for each coordinate of its `n` points.
        let fractions = uor_fractions(e);
        let dims = if d3 { 3 } else { 2 };
        let vertex = |k: usize, at: usize, n: usize| -> Option<[f64; 3]> {
            let mut p = pt3(at)?;
            if fractions.len() == n * dims {
                for (j, c) in p.iter_mut().take(dims).enumerate() {
                    *c += fractions[k * dims + j];
                }
            }
            Some(p)
        };
        match kind {
            // A cell: its components follow as complex elements, as many as
            // its total length (36, the words after its first 38 bytes)
            // covers; they are already placed, so the cell keeps them as
            // they are. Nesting past `MAX_CELL_DEPTH` is left out.
            2 => {
                let total = 38 + uint16(e, 36).unwrap_or(0) as usize * 2;
                let mut left = total.saturating_sub(e.len());
                let start = index;
                while left > 0 && index < run.len() && run[index][0] & 0x80 != 0 {
                    left = left.saturating_sub(run[index].len());
                    index += 1;
                }
                if depth >= MAX_CELL_DEPTH {
                    continue;
                }
                let inner = read_run(&run[start..index], colors, d3, depth + 1, code_page);
                raw.paths.extend(inner.paths);
                raw.texts.extend(inner.texts);
                raw.instances.extend(inner.instances);
                // The origin: 84 (112 in 3D), after the range and the
                // transformation.
                collect.push(element(DgnGeom::CellGroup {
                    name: radix50(e, 38),
                    origin: pt3(if d3 { 112 } else { 84 }).unwrap_or([0.0; 3]),
                    parts: inner.elements,
                }));
            }
            // A text node: its count of text components, justification
            // (45), line spacing (46) and origin (62, or 74 in 3D).
            7 => collect.open(
                uint16(e, 38).unwrap_or(0) as usize,
                element(DgnGeom::TextNode {
                    lines: Vec::new(),
                    origin: pt3(if d3 { 74 } else { 62 }),
                    just: e.get(45).copied().unwrap_or(0),
                    line_spacing: int32(e, 46).map(|v| v as f64),
                }),
            ),
            12 | 14 => {
                let n = uint16(e, 38).unwrap_or(0) as usize;
                collect.open(n, element(DgnGeom::Chain(Vec::new())));
                if n > 0 {
                    complex = Some((
                        n,
                        PathBuilder::default(),
                        stroke,
                        if kind == 14 { fill } else { None },
                        kind == 14,
                    ));
                }
            }
            3 => {
                if let (Some(a3), Some(b3)) = (vertex(0, 36, 2), vertex(1, 36 + step, 2)) {
                    let (a, b) = (plan(a3), plan(b3));
                    collect.push(element(DgnGeom::Line(a3, b3)));
                    push(
                        &mut raw,
                        &mut complex,
                        Path {
                            subpaths: vec![SubPath {
                                segments: vec![Segment::Line(a, b)],
                                closed: false,
                            }],
                            stroke,
                            fill: None,
                        },
                    );
                }
            }
            4 | 6 | 11 => {
                let n = uint16(e, 36).unwrap_or(0) as usize;
                let pts3: Vec<[f64; 3]> =
                    (0..n).filter_map(|k| vertex(k, 38 + k * step, n)).collect();
                let pts: Vec<[f64; 2]> = pts3.iter().map(|p| plan(*p)).collect();
                if pts.len() >= 2 {
                    collect.push(element(if kind == 11 {
                        DgnGeom::Curve(pts3)
                    } else {
                        DgnGeom::Points(pts3)
                    }));
                    push(
                        &mut raw,
                        &mut complex,
                        polyline(&pts, kind == 6, stroke, if kind == 6 { fill } else { None }),
                    );
                }
            }
            // 3D ellipses and arcs: the axes from the quaternion (52 or
            // 60), the centre as three doubles after it.
            15 | 16 if d3 => {
                let at = if kind == 15 { 36 } else { 44 };
                let q = at + 16;
                let (Some(a), Some(b), Some(axes), Some(cx), Some(cy), Some(cz)) = (
                    vax_double(e, at),
                    vax_double(e, at + 8),
                    quaternion_axes(e, q),
                    vax_double(e, q + 16),
                    vax_double(e, q + 24),
                    vax_double(e, q + 32),
                ) else {
                    continue;
                };
                let (start, sweep) = if kind == 15 {
                    (0.0, 360.0)
                } else {
                    let (Some(st), Some(sw)) = (int32(e, 36), int32(e, 40)) else {
                        continue;
                    };
                    let sw = sw as u32;
                    let mut sweep = if sw & 0x8000_0000 != 0 {
                        -((sw & 0x7fff_ffff) as f64)
                    } else {
                        sw as f64
                    } / 360000.0;
                    if sweep == 0.0 {
                        sweep = 360.0;
                    }
                    (st as f64 / 360000.0, sweep)
                };
                let [x, y, _] = axes;
                collect.push(element(DgnGeom::Arc {
                    center: [cx, cy, cz],
                    u: x.map(|c| c * a),
                    v: y.map(|c| c * b),
                    start: start.to_radians(),
                    sweep: sweep.to_radians(),
                }));
                let pieces = arc_cubics(
                    [cx, cy],
                    a,
                    b,
                    x[1].atan2(x[0]),
                    start.to_radians(),
                    sweep.to_radians(),
                );
                let closed = sweep.abs() >= 360.0;
                push(
                    &mut raw,
                    &mut complex,
                    Path {
                        subpaths: vec![SubPath {
                            segments: pieces
                                .into_iter()
                                .map(|[a, b, c, d]| Segment::Cubic(a, b, c, d))
                                .collect(),
                            closed,
                        }],
                        stroke,
                        fill: if closed { fill } else { None },
                    },
                );
            }
            15 | 16 => {
                let arc = if kind == 15 {
                    (|| {
                        Some((
                            vax_double(e, 36)?,
                            vax_double(e, 44)?,
                            int32(e, 52)?,
                            vax_double(e, 56)?,
                            vax_double(e, 64)?,
                            0.0,
                            360.0,
                        ))
                    })()
                } else {
                    (|| {
                        let sw = int32(e, 40)? as u32;
                        let mut sweep = if sw & 0x8000_0000 != 0 {
                            -((sw & 0x7fff_ffff) as f64)
                        } else {
                            sw as f64
                        } / 360000.0;
                        if sweep == 0.0 {
                            sweep = 360.0;
                        }
                        Some((
                            vax_double(e, 44)?,
                            vax_double(e, 52)?,
                            int32(e, 60)?,
                            vax_double(e, 64)?,
                            vax_double(e, 72)?,
                            int32(e, 36)? as f64 / 360000.0,
                            sweep,
                        ))
                    })()
                };
                let Some((a, b, r, ox, oy, start, sweep)) = arc else {
                    continue;
                };
                let turn = (r as f64 / 360000.0).to_radians();
                let (c, s) = (turn.cos(), turn.sin());
                collect.push(element(DgnGeom::Arc {
                    center: [ox, oy, 0.0],
                    u: [a * c, a * s, 0.0],
                    v: [-b * s, b * c, 0.0],
                    start: start.to_radians(),
                    sweep: sweep.to_radians(),
                }));
                let pieces = arc_cubics(
                    [ox, oy],
                    a,
                    b,
                    (r as f64 / 360000.0).to_radians(),
                    start.to_radians(),
                    sweep.to_radians(),
                );
                let closed = sweep.abs() >= 360.0;
                push(
                    &mut raw,
                    &mut complex,
                    Path {
                        subpaths: vec![SubPath {
                            segments: pieces
                                .into_iter()
                                .map(|[a, b, c, d]| Segment::Cubic(a, b, c, d))
                                .collect(),
                            closed,
                        }],
                        stroke,
                        fill: if closed { fill } else { None },
                    },
                );
            }
            17 => {
                // 2D: rotation (46), origin (50), count (58), characters
                // (60); 3D: quaternion (46), origin (62), count (74),
                // characters (76).
                let rotation = if d3 {
                    quaternion_axes(e, 46)
                        .map(|[x, _, _]| (x[1].atan2(x[0]).to_degrees() * 360000.0).round() as i32)
                } else {
                    int32(e, 46)
                };
                let (origin_at, count_at) = if d3 { (62, 74) } else { (50, 58) };
                let (Some(width), Some(height), Some(rotation), Some(origin3)) =
                    (int32(e, 38), int32(e, 42), rotation, pt3(origin_at))
                else {
                    continue;
                };
                let origin = plan(origin3);
                let n = e.get(count_at).copied().unwrap_or(0) as usize;
                let Some(chars) = e.get(count_at + 2..count_at + 2 + n) else {
                    continue;
                };
                // Sizes in UORs (the multipliers are 1000/6 of one), not
                // rounded.
                let (height, width) = (height as f64 * 6.0 / 1000.0, width as f64 * 6.0 / 1000.0);
                let text = Text {
                    text: single_byte_text(chars, code_page)
                        .trim_end_matches('\0')
                        .to_string(),
                    origin,
                    height,
                    width_factor: if height > 0.0 { width / height } else { 1.0 },
                    rotation: (rotation as f64 / 360000.0).to_radians(),
                    color,
                    font: "txt".to_string(),
                };
                // A V7 font is drawn monospaced: the text is as long as its
                // characters times their width.
                collect.push(element(DgnGeom::Text {
                    text: text.text.clone(),
                    origin: origin3,
                    height,
                    width_factor: text.width_factor,
                    length: text.text.chars().count() as f64 * width,
                    rotation: text.rotation,
                    just: e[37],
                    font: Some(e[36] as u32),
                    style: None,
                }));
                raw.texts.push(text);
            }
            27 => {
                // B-spline curve header: order and closure (40), pole count
                // (42), knot count (44); the curve imports with the header's
                // level and symbology.
                let flags = e.get(40).copied().unwrap_or(0);
                let order = ((flags & 0x0f) + 2) as usize;
                let poles = uint16(e, 42).unwrap_or(0) as usize;
                let header = element(DgnGeom::Chain(Vec::new()));
                spline = Some((order, poles, color, flags & 0x80 != 0, header));
                knots = vec![f64::NAN; uint16(e, 44).unwrap_or(0) as usize];
            }
            26 => {
                // Knots as fractions of the parameter range; a closed
                // curve's list also holds its ends.
                let n = knots.len();
                knots = (0..n)
                    .filter_map(|k| int32(e, 36 + k * 4))
                    .map(|v| v as u32 as f64 / 2_147_483_648.0)
                    .collect();
            }
            21 => {
                let n = uint16(e, 36).unwrap_or(0) as usize;
                let poles3: Vec<[f64; 3]> = (0..n).filter_map(|k| pt3(38 + k * step)).collect();
                let poles: Vec<[f64; 2]> = poles3.iter().map(|p| plan(*p)).collect();
                if let Some((order, count, color, closed, header)) = spline.take() {
                    if poles.len() == count && count >= order {
                        let stored: Vec<f64> = knots
                            .iter()
                            .copied()
                            .filter(|k| *k > 1e-9 && *k < 1.0 - 1e-6)
                            .collect();
                        let interior = if stored.len() == count - order {
                            stored
                        } else {
                            (1..=count - order)
                                .map(|k| k as f64 / (count - order + 1) as f64)
                                .collect()
                        };
                        let mut full = vec![0.0; order];
                        full.extend(interior);
                        full.extend(vec![1.0; order]);
                        let pts = bspline_points(order, &poles, &full, 24 * count);
                        collect.push(DgnElement {
                            geom: DgnGeom::Spline {
                                degree: order - 1,
                                knots: full,
                                poles: poles3.clone(),
                                closed,
                            },
                            ..header
                        });
                        push(
                            &mut raw,
                            &mut complex,
                            polyline(&pts, false, Some((color, -1.0)), None),
                        );
                    }
                }
            }
            35 => {
                // Shared cell instance: the definition's name, a 2×2 matrix
                // and the origin.
                let name = e
                    .get(164..180)
                    .map(|n| {
                        String::from_utf8_lossy(n)
                            .trim_end_matches('\0')
                            .trim()
                            .to_string()
                    })
                    .unwrap_or_default();
                // A 3D instance holds a 3×3 matrix (its plan part and the z
                // scale are used) and a z.
                let (r1, zz) = if d3 { (100, 140) } else { (92, 0) };
                let (Some(m0), Some(m1), Some(m2), Some(m3), Some(ox), Some(oy)) = (
                    vax_double(e, 76),
                    vax_double(e, 84),
                    vax_double(e, r1),
                    vax_double(e, r1 + 8),
                    le_i32(e, 148),
                    le_i32(e, 152),
                ) else {
                    continue;
                };
                let (oz, z_scale) = if d3 {
                    (
                        le_i32(e, 156).unwrap_or(0) as f64,
                        vax_double(e, zz).filter(|z| *z != 0.0).unwrap_or(1.0),
                    )
                } else {
                    (0.0, 1.0)
                };
                collect.push(element(DgnGeom::Cell {
                    name: name.clone(),
                    m: [m0, m1, m2, m3],
                    origin: [ox as f64, oy as f64, oz],
                    z_scale,
                }));
                raw.instances.push(Instance {
                    name,
                    m: [m0, m1, m2, m3],
                    origin: [ox as f64, oy as f64],
                });
            }
            _ => {}
        }
    }
    collect.close();
    raw.elements = collect.elements;
    raw
}

fn polyline(
    pts: &[[f64; 2]],
    closed: bool,
    stroke: Option<([u8; 3], f64)>,
    fill: Option<[u8; 3]>,
) -> Path {
    let mut pb = PathBuilder::default();
    pb.move_to(pts[0]);
    for p in &pts[1..] {
        pb.line_to(*p);
    }
    if closed {
        pb.close();
    }
    Path {
        subpaths: pb.finish(),
        stroke,
        fill,
    }
}

/// x' = a·x + b·y + e, y' = c·x + d·y + f.
type Affine = [f64; 6];

fn apply(m: &Affine, p: [f64; 2]) -> [f64; 2] {
    [
        m[0] * p[0] + m[1] * p[1] + m[4],
        m[2] * p[0] + m[3] * p[1] + m[5],
    ]
}

fn compose(outer: &Affine, inner: &Affine) -> Affine {
    [
        outer[0] * inner[0] + outer[1] * inner[2],
        outer[0] * inner[1] + outer[1] * inner[3],
        outer[2] * inner[0] + outer[3] * inner[2],
        outer[2] * inner[1] + outer[3] * inner[3],
        outer[0] * inner[4] + outer[1] * inner[5] + outer[4],
        outer[2] * inner[4] + outer[3] * inner[5] + outer[5],
    ]
}

/// Places raw geometry (and the shared cells it instances) through `m`.
fn place(raw: &Raw, m: &Affine, defs: &HashMap<String, Raw>, depth: usize, out: &mut Sheet) {
    let map = |p: [f64; 2]| apply(m, p);
    for path in &raw.paths {
        out.paths.push(Path {
            subpaths: path
                .subpaths
                .iter()
                .map(|sp| SubPath {
                    segments: sp
                        .segments
                        .iter()
                        .map(|s| match s {
                            Segment::Line(a, b) => Segment::Line(map(*a), map(*b)),
                            Segment::Cubic(a, b, c, d) => {
                                Segment::Cubic(map(*a), map(*b), map(*c), map(*d))
                            }
                        })
                        .collect(),
                    closed: sp.closed,
                })
                .collect(),
            stroke: path.stroke,
            fill: path.fill,
        });
    }
    let scale = (m[0] * m[3] - m[1] * m[2]).abs().sqrt();
    let turn = m[2].atan2(m[0]);
    for t in &raw.texts {
        out.texts.push(Text {
            origin: map(t.origin),
            height: t.height * scale,
            rotation: t.rotation + turn,
            ..t.clone()
        });
    }
    if depth >= 8 {
        return;
    }
    for inst in &raw.instances {
        let Some(def) = defs.get(&inst.name.to_ascii_uppercase()) else {
            continue;
        };
        let local: Affine = [
            inst.m[0],
            inst.m[1],
            inst.m[2],
            inst.m[3],
            inst.origin[0],
            inst.origin[1],
        ];
        place(def, &compose(m, &local), defs, depth + 1, out);
    }
}

/// A level's number (0-63) from an element header.
fn level(e: &[u8]) -> u8 {
    e[0] & 0x3f
}

/// Level names from the level name table (type 66 elements on level 6):
/// each entry is a level number followed by its name.
// ponytail: entries are found by that pattern rather than by the table's
// record layout; read the layout if a file lists a level wrongly.
fn level_names(all: &[&[u8]], code_page: Option<u16>) -> HashMap<u8, String> {
    let mut names = HashMap::new();
    for e in all.iter().filter(|e| e[1] & 0x7f == 66 && level(e) == 6) {
        // The named-levels table: a count at 40, then 104-byte entries of
        // the level number and its zero-terminated name (which may hold
        // spaces, as "Level 1").
        let byte = |at: usize| e.get(at).copied().unwrap_or(0);
        let count = usize::from(u16::from_le_bytes([byte(40), byte(41)]));
        if count > 0 && 42 + count * 104 <= e.len() {
            for k in 0..count {
                let at = 42 + k * 104;
                let n = u16::from_le_bytes([e[at], e[at + 1]]);
                let field = &e[at + 2..at + 104];
                let len = field.iter().position(|c| *c == 0).unwrap_or(field.len());
                let name = single_byte_text(&field[..len], code_page);
                if (1..=63).contains(&n) && !name.trim().is_empty() {
                    names
                        .entry(n as u8)
                        .or_insert_with(|| name.trim().to_string());
                }
            }
            continue;
        }
        for j in 36..e.len().saturating_sub(3) {
            let n = u16::from_le_bytes([e[j], e[j + 1]]);
            if !(1..=63).contains(&n) || (j > 36 && e[j - 1] != 0) || names.contains_key(&(n as u8))
            {
                continue;
            }
            let rest = &e[j + 2..];
            let len = rest
                .iter()
                .position(|c| !(c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'$')))
                .unwrap_or(rest.len());
            if len > 0 && rest.get(len) == Some(&0) {
                names.insert(n as u8, String::from_utf8_lossy(&rest[..len]).to_string());
            }
        }
    }
    names
}

/// The used levels' names (or numbers), for the underlay layers list.
pub fn layer_names(bytes: &[u8], code_page: Option<u16>) -> Option<Vec<String>> {
    if !is_v7(bytes) {
        return None;
    }
    let all = elements(bytes);
    let names = level_names(&all, code_page);
    let mut used: Vec<u8> = all
        .iter()
        .filter(|e| !matches!(e[1] & 0x7f, 9 | 8 | 10 | 5 | 66 | 34) && level(e) > 0)
        .map(|e| level(e))
        .collect();
    used.sort_unstable();
    used.dedup();
    Some(
        used.into_iter()
            .map(|l| names.get(&l).cloned().unwrap_or_else(|| l.to_string()))
            .collect(),
    )
}

/// The model's geometry, in master units, without the `hidden` levels.
pub fn model(bytes: &[u8], hidden: &[String], code_page: Option<u16>) -> Option<Sheet> {
    if !is_v7(bytes) {
        return None;
    }
    let u = units(bytes)?;
    let all = elements(bytes);
    let table = color_table(&all);
    let table_colors = table.is_some();
    let colors = table.unwrap_or_else(|| DGN_DEFAULT_COLORS.to_vec());
    let names = level_names(&all, code_page);
    let off = |e: &[u8]| {
        let l = level(e);
        let name = names.get(&l).cloned().unwrap_or_else(|| l.to_string());
        hidden.iter().any(|h| h.eq_ignore_ascii_case(&name))
    };
    let all: Vec<&[u8]> = all.into_iter().filter(|e| !off(e)).collect();
    // Shared cell definitions (type 34) and their components are drawn only
    // where instances place them; everything else is the model itself.
    let mut defs: HashMap<String, Raw> = HashMap::new();
    let mut top: Vec<&[u8]> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        let e = all[i];
        if e[1] & 0x7f == 34 && e[0] & 0x80 == 0 {
            let name = e
                .get(164..180)
                .map(|n| {
                    String::from_utf8_lossy(n)
                        .trim_end_matches('\0')
                        .trim()
                        .to_ascii_uppercase()
                })
                .unwrap_or_default();
            let mut j = i + 1;
            while j < all.len() && all[j][0] & 0x80 != 0 {
                j += 1;
            }
            defs.insert(
                name,
                read_run(&all[i + 1..j], &colors, is_3d(bytes), 0, code_page),
            );
            i = j;
            continue;
        }
        top.push(e);
        i += 1;
    }
    let raw = read_run(&top, &colors, is_3d(bytes), 0, code_page);
    let per_unit = u.uor_per_sub * u.sub_per_master;
    let m: Affine = [
        1.0 / per_unit,
        0.0,
        0.0,
        1.0 / per_unit,
        -u.origin[0] / per_unit,
        -u.origin[1] / per_unit,
    ];
    let mut sheet = Sheet {
        sub_per_master: u.sub_per_master,
        table_colors,
        ..Default::default()
    };
    place(&raw, &m, &defs, 0, &mut sheet);
    sheet.rect = paths_bounds(&sheet.paths).or_else(|| {
        let t = sheet.texts.first()?;
        Some([
            t.origin[0],
            t.origin[1],
            t.origin[0] + t.height,
            t.origin[1] + t.height,
        ])
    })?;
    Some(sheet)
}

/// A unit name of the design file header (two characters, zero padded).
fn unit_name(b: &[u8], at: usize) -> String {
    b.get(at..at + 2)
        .map(|n| {
            String::from_utf8_lossy(n)
                .trim_matches(char::from(0))
                .trim()
                .to_string()
        })
        .unwrap_or_default()
}

/// The model element by element (DGNIMPORT): in master units with the
/// global origin at 0,0, shared cell definitions in their own units scaled
/// to master units, and every level by number with its name.
pub fn import(bytes: &[u8], code_page: Option<u16>) -> Option<DgnImport> {
    if !is_v7(bytes) {
        return None;
    }
    let u = units(bytes)?;
    let all = elements(bytes);
    let colors = color_table(&all).unwrap_or_else(|| DGN_DEFAULT_COLORS.to_vec());
    let names = level_names(&all, code_page);
    let per_unit = u.uor_per_sub * u.sub_per_master;
    let s = 1.0 / per_unit;
    let mut cells = Vec::new();
    let mut top: Vec<&[u8]> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        let e = all[i];
        if e[1] & 0x7f == 34 && e[0] & 0x80 == 0 {
            let name = e
                .get(164..180)
                .map(|n| {
                    String::from_utf8_lossy(n)
                        .trim_end_matches('\0')
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();
            let mut j = i + 1;
            while j < all.len() && all[j][0] & 0x80 != 0 {
                j += 1;
            }
            let def = read_run(&all[i + 1..j], &colors, is_3d(bytes), 0, code_page).elements;
            cells.push((name, scaled_elements(&def, s, [0.0; 3])));
            i = j;
            continue;
        }
        top.push(e);
        i += 1;
    }
    let raw = read_run(&top, &colors, is_3d(bytes), 0, code_page);
    let levels = (0..64u32)
        .map(|l| {
            let name = names
                .get(&(l as u8))
                .cloned()
                .unwrap_or_else(|| l.to_string());
            (l, (name, None))
        })
        .collect();
    Some(DgnImport {
        dim_styles: Vec::new(),
        fonts: Vec::new(),
        three_d: is_3d(bytes),
        elements: scaled_elements(&raw.elements, s, [-u.origin[0] * s, -u.origin[1] * s, 0.0]),
        cells,
        levels,
        sub_per_master: u.sub_per_master,
        unit_names: (unit_name(bytes, 1120), unit_name(bytes, 1122)),
        v7: true,
        // The first view's flags (46): constructions shown at 0x1000.
        construction_layers: uint16(bytes, 46).is_some_and(|flags| flags & 0x1000 == 0),
        ..Default::default()
    })
}

#[cfg(test)]
mod text_code_page_tests {
    use super::*;

    /// The report's own byte evidence: Czech level/text bytes written by
    /// MicroStation V7 on a Windows-1250 system decode through the page, not
    /// through 1252.
    #[test]
    fn windows_1250_decodes_czech_slovak_text() {
        let cases: &[(&[u8], u16, &str)] = &[
            (b"Zna\xE8ky", 1250, "Značky"),
            (b"Defini\xE8n\xED bod k\xFA", 1250, "Definiční bod kú"),
            (
                b"V\xEccn\xE1 b\xF8emena k \xE8\xE1sti",
                1250,
                "Věcná břemena k části",
            ),
            // Bytes the page leaves undefined stand for themselves.
            (
                [0x81, 0x83, 0x88, 0x90, 0x98].as_slice(),
                1250,
                "\u{81}\u{83}\u{88}\u{90}\u{98}",
            ),
            // None keeps the historical 1252 decoding.
            (b"Zna\xE8ky".as_slice(), 1252, "Znaèky"),
            (b"Zna\xE8ky".as_slice(), 9999 as u16, "Znaèky"),
        ];
        for (bytes, page, text) in cases {
            assert_eq!(single_byte_text(bytes, Some(*page)), *text, "page {page}");
        }
    }

    /// The pages behind the import window's picker decode their typical
    /// scripts through `encoding_rs`; the default stays 1252.
    #[test]
    fn chosen_windows_pages_decode_their_scripts() {
        let cases: &[(&[u8], u16, &str)] = &[
            // Cyrillic: Windows-1251.
            ([0xCF, 0xF0, 0xE8].as_slice(), 1251, "При"),
            // Greek: Windows-1253.
            ([0xC1, 0xE2].as_slice(), 1253, "Αβ"),
            // Turkish dotted capital I: Windows-1254.
            ([0xDD].as_slice(), 1254, "İ"),
            // Baltic: Windows-1257.
            ([0xE0].as_slice(), 1257, "ą"),
            // Undefined bytes decode through what the standard tables give;
            // 1252 keeps its own historical table (0x98 is small tilde).
            ([0x98].as_slice(), 1251, "\u{98}"),
            ([0x98].as_slice(), 1252, "\u{2DC}"),
        ];
        for (bytes, page, text) in cases {
            assert_eq!(single_byte_text(bytes, Some(*page)), *text, "page {page}");
        }
    }

    /// A level-name entry of the type-66 table decodes with the page, too.
    #[test]
    fn level_table_names_decode_with_the_page() {
        let entry = |name: &[u8]| {
            let mut e = vec![0u8; 42 + 104];
            // Level 6 element (type 66) carrying one named level.
            e[0] = 6;
            e[1] = 66;
            e[40] = 1u8;
            e[42] = 5u8;
            e[44..44 + name.len()].copy_from_slice(name);
            e
        };
        for (name, page, text) in [
            (b"Zna\xE8ky budov".as_slice(), 1250, "Značky budov"),
            (b"Vnit\xF8n\xED kresba".as_slice(), 1250, "Vnitřní kresba"),
            (b"Zna\xE8ky budov".as_slice(), 1252, "Znaèky budov"),
        ] {
            let element = entry(name);
            let names = level_names(&[&element], Some(page));
            assert_eq!(names.get(&5).map(String::as_str), Some(text), "page {page}");
        }
    }

    /// A kind-17 (2-D text) element decodes through `read_run` with the page.
    #[test]
    fn text_elements_decode_with_the_page() {
        let element = |chars: &[u8]| {
            let mut e = vec![0u8; 60 + chars.len() + 2];
            // Level 6, kind 17, not a complex component.
            e[0] = 6;
            e[1] = 17;
            // Height 1000 UORs, width factor 500/1000.
            e[38] = 0x03;
            e[39] = 0xE8;
            e[42] = 0x01;
            e[43] = 0xF4;
            let n = chars.len() as u16;
            e[58] = (n & 0xff) as u8;
            e[59] = (n >> 8) as u8;
            e[60..60 + chars.len()].copy_from_slice(chars);
            e
        };
        for (chars, page, text) in [
            (b"Zna\xE8ky".as_slice(), 1250, "Značky"),
            (b"Zna\xE8ky".as_slice(), 1252, "Znaèky"),
        ] {
            let element = element(chars);
            let raw = read_run(&[&element], &[[8, 8, 8]], false, 0, Some(page));
            assert_eq!(
                raw.texts.first().map(|t| t.text.as_str()),
                Some(text),
                "page {page}"
            );
        }
    }
}
