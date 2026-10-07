//! DGN V7 (ISFF) design files: one model ("Default") of variable-length
//! element records. Coordinates are UORs; the design file header (TCB)
//! gives the global origin and the master unit the model is read in. Shared
//! cells are drawn where their instances place them.

use std::collections::HashMap;

use super::model::{arc_cubics, bspline_points, dgn_color_table, paths_bounds, Path, PathBuilder, Segment, Sheet, SubPath, Text, DGN_DEFAULT_COLORS};

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

struct Units {
    uor_per_sub: f64,
    sub_per_master: f64,
    origin: [f64; 2],
}

fn units(b: &[u8]) -> Option<Units> {
    let sub_per_master = int32(b, 1112)? as f64;
    let uor_per_sub = int32(b, 1116)? as f64;
    let origin = [vax_double(b, 1240)?, vax_double(b, 1248)?];
    (sub_per_master > 0.0 && uor_per_sub > 0.0).then(|| Units {
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
        let Some(words) = uint16(b, at + 2) else { break };
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
    elements
        .iter()
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
}

#[derive(Clone)]
struct Instance {
    name: String,
    /// x' = m[0]·x + m[1]·y + origin.x, y' = m[2]·x + m[3]·y + origin.y.
    m: [f64; 4],
    origin: [f64; 2],
}

/// Reads a run of elements into raw geometry: plain elements, complex
/// chains and shapes (joined), B-splines, text and shared cell instances.
fn read_run(run: &[&[u8]], colors: &[[u8; 3]]) -> Raw {
    let mut raw = Raw::default();
    let mut spline: Option<(usize, usize, [u8; 3], f64)> = None;
    let mut knots: Vec<f64> = Vec::new();
    // A complex shape or chain collects its components into one path.
    let mut complex: Option<(usize, PathBuilder, Option<([u8; 3], f64)>, Option<[u8; 3]>, bool)> = None;
    let push = |raw: &mut Raw, complex: &mut Option<(usize, PathBuilder, Option<([u8; 3], f64)>, Option<[u8; 3]>, bool)>, path: Path| {
        if let Some((left, pb, _, _, _)) = complex.as_mut() {
            for sp in &path.subpaths {
                for seg in &sp.segments {
                    match seg {
                        Segment::Line(a, b) => {
                            if pb.current() != Some(*a) {
                                if pb.current().is_none() { pb.move_to(*a) } else { pb.line_to(*a) }
                            }
                            pb.line_to(*b);
                        }
                        Segment::Cubic(a, c1, c2, b) => {
                            if pb.current() != Some(*a) {
                                if pb.current().is_none() { pb.move_to(*a) } else { pb.line_to(*a) }
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
                raw.paths.push(Path { subpaths: pb.finish(), stroke, fill });
            }
        } else {
            raw.paths.push(path);
        }
    };
    for e in run {
        let kind = e[1] & 0x7f;
        if e[1] & 0x80 != 0 || e.len() < 36 {
            continue;
        }
        let symbology = uint16(e, 34).unwrap_or(0);
        let color = colors[(symbology >> 8) as usize];
        // One pixel wide, as V8 models draw: the element weight is a
        // lineweight, not shown while lineweights are off.
        let stroke = Some((color, -1.0));
        let fill = fill_index(e).map(|i| colors[i as usize]);
        let int_pt = |i: usize| -> Option<[f64; 2]> { Some([int32(e, i)? as f64, int32(e, i + 4)? as f64]) };
        match kind {
            12 | 14 => {
                let n = uint16(e, 38).unwrap_or(0) as usize;
                if n > 0 {
                    complex = Some((n, PathBuilder::default(), stroke, if kind == 14 { fill } else { None }, kind == 14));
                }
            }
            3 => {
                if let (Some(a), Some(b)) = (int_pt(36), int_pt(44)) {
                    push(&mut raw, &mut complex, Path { subpaths: vec![SubPath { segments: vec![Segment::Line(a, b)], closed: false }], stroke, fill: None });
                }
            }
            4 | 6 | 11 => {
                let n = uint16(e, 36).unwrap_or(0) as usize;
                let pts: Vec<[f64; 2]> = (0..n).filter_map(|k| int_pt(38 + k * 8)).collect();
                if pts.len() >= 2 {
                    push(&mut raw, &mut complex, polyline(&pts, kind == 6, stroke, if kind == 6 { fill } else { None }));
                }
            }
            15 | 16 => {
                let arc = if kind == 15 {
                    (|| Some((vax_double(e, 36)?, vax_double(e, 44)?, int32(e, 52)?, vax_double(e, 56)?, vax_double(e, 64)?, 0.0, 360.0)))()
                } else {
                    (|| {
                        let sw = int32(e, 40)? as u32;
                        let mut sweep = if sw & 0x8000_0000 != 0 { -((sw & 0x7fff_ffff) as f64) } else { sw as f64 } / 360000.0;
                        if sweep == 0.0 {
                            sweep = 360.0;
                        }
                        Some((vax_double(e, 44)?, vax_double(e, 52)?, int32(e, 60)?, vax_double(e, 64)?, vax_double(e, 72)?, int32(e, 36)? as f64 / 360000.0, sweep))
                    })()
                };
                let Some((a, b, r, ox, oy, start, sweep)) = arc else { continue };
                let pieces = arc_cubics([ox, oy], a, b, (r as f64 / 360000.0).to_radians(), start.to_radians(), sweep.to_radians());
                let closed = sweep.abs() >= 360.0;
                push(
                    &mut raw,
                    &mut complex,
                    Path {
                        subpaths: vec![SubPath { segments: pieces.into_iter().map(|[a, b, c, d]| Segment::Cubic(a, b, c, d)).collect(), closed }],
                        stroke,
                        fill: if closed { fill } else { None },
                    },
                );
            }
            17 => {
                let (Some(width), Some(height), Some(rotation), Some(origin)) = (int32(e, 38), int32(e, 42), int32(e, 46), int_pt(50)) else {
                    continue;
                };
                let n = e.get(58).copied().unwrap_or(0) as usize;
                let Some(chars) = e.get(60..60 + n) else { continue };
                let (height, width) = (height as f64 * 6.0 / 1000.0, width as f64 * 6.0 / 1000.0);
                raw.texts.push(Text {
                    text: String::from_utf8_lossy(chars).trim_end_matches('\0').to_string(),
                    origin,
                    height,
                    width_factor: if height > 0.0 { width / height } else { 1.0 },
                    rotation: (rotation as f64 / 360000.0).to_radians(),
                    color,
                    font: "txt".to_string(),
                });
            }
            27 => {
                // B-spline curve header: order, pole count.
                let order = ((e.get(40).copied().unwrap_or(0) & 0x0f) + 2) as usize;
                let poles = uint16(e, 42).unwrap_or(0) as usize;
                spline = Some((order, poles, color, -1.0));
                knots.clear();
            }
            26 => {
                // Interior knots as fractions of the parameter range.
                let n = spline.map(|s| s.1.saturating_sub(s.0)).unwrap_or(0);
                knots = (0..n).filter_map(|k| int32(e, 36 + k * 4)).map(|v| v as u32 as f64 / 2_147_483_648.0).collect();
            }
            21 => {
                let n = uint16(e, 36).unwrap_or(0) as usize;
                let poles: Vec<[f64; 2]> = (0..n).filter_map(|k| int_pt(38 + k * 8)).collect();
                if let Some((order, count, color, width)) = spline.take() {
                    if poles.len() == count && count >= order {
                        let interior = if knots.len() == count - order {
                            knots.clone()
                        } else {
                            (1..=count - order).map(|k| k as f64 / (count - order + 1) as f64).collect()
                        };
                        let mut full = vec![0.0; order];
                        full.extend(interior);
                        full.extend(vec![1.0; order]);
                        let pts = bspline_points(order, &poles, &full, 24 * count);
                        push(&mut raw, &mut complex, polyline(&pts, false, Some((color, width)), None));
                    }
                }
            }
            35 => {
                // Shared cell instance: the definition's name, a 2×2 matrix
                // and the origin.
                let name = e.get(164..180).map(|n| String::from_utf8_lossy(n).trim_end_matches('\0').trim().to_string()).unwrap_or_default();
                let (Some(m0), Some(m1), Some(m2), Some(m3), Some(ox), Some(oy)) =
                    (vax_double(e, 76), vax_double(e, 84), vax_double(e, 92), vax_double(e, 100), le_i32(e, 148), le_i32(e, 152))
                else {
                    continue;
                };
                raw.instances.push(Instance { name, m: [m0, m1, m2, m3], origin: [ox as f64, oy as f64] });
            }
            _ => {}
        }
    }
    raw
}

fn polyline(pts: &[[f64; 2]], closed: bool, stroke: Option<([u8; 3], f64)>, fill: Option<[u8; 3]>) -> Path {
    let mut pb = PathBuilder::default();
    pb.move_to(pts[0]);
    for p in &pts[1..] {
        pb.line_to(*p);
    }
    if closed {
        pb.close();
    }
    Path { subpaths: pb.finish(), stroke, fill }
}

/// x' = a·x + b·y + e, y' = c·x + d·y + f.
type Affine = [f64; 6];

fn apply(m: &Affine, p: [f64; 2]) -> [f64; 2] {
    [m[0] * p[0] + m[1] * p[1] + m[4], m[2] * p[0] + m[3] * p[1] + m[5]]
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
                            Segment::Cubic(a, b, c, d) => Segment::Cubic(map(*a), map(*b), map(*c), map(*d)),
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
        out.texts.push(Text { origin: map(t.origin), height: t.height * scale, rotation: t.rotation + turn, ..t.clone() });
    }
    if depth >= 8 {
        return;
    }
    for inst in &raw.instances {
        let Some(def) = defs.get(&inst.name.to_ascii_uppercase()) else { continue };
        let local: Affine = [inst.m[0], inst.m[1], inst.m[2], inst.m[3], inst.origin[0], inst.origin[1]];
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
fn level_names(all: &[&[u8]]) -> HashMap<u8, String> {
    let mut names = HashMap::new();
    for e in all.iter().filter(|e| e[1] & 0x7f == 66 && level(e) == 6) {
        for j in 36..e.len().saturating_sub(3) {
            let n = u16::from_le_bytes([e[j], e[j + 1]]);
            if !(1..=63).contains(&n) || (j > 36 && e[j - 1] != 0) || names.contains_key(&(n as u8)) {
                continue;
            }
            let rest = &e[j + 2..];
            let len = rest.iter().position(|c| !(c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'$'))).unwrap_or(rest.len());
            if len > 0 && rest.get(len) == Some(&0) {
                names.insert(n as u8, String::from_utf8_lossy(&rest[..len]).to_string());
            }
        }
    }
    names
}

/// The used levels' names (or numbers), for the underlay layers list.
pub fn layer_names(bytes: &[u8]) -> Option<Vec<String>> {
    if !is_v7(bytes) {
        return None;
    }
    let all = elements(bytes);
    let names = level_names(&all);
    let mut used: Vec<u8> = all
        .iter()
        .filter(|e| !matches!(e[1] & 0x7f, 9 | 8 | 10 | 5 | 66 | 34) && level(e) > 0)
        .map(|e| level(e))
        .collect();
    used.sort_unstable();
    used.dedup();
    Some(used.into_iter().map(|l| names.get(&l).cloned().unwrap_or_else(|| l.to_string())).collect())
}

/// The model's geometry, in master units, without the `hidden` levels.
pub fn model(bytes: &[u8], hidden: &[String]) -> Option<Sheet> {
    if !is_v7(bytes) {
        return None;
    }
    let u = units(bytes)?;
    let all = elements(bytes);
    let table = color_table(&all);
    let table_colors = table.is_some();
    let colors = table.unwrap_or_else(|| DGN_DEFAULT_COLORS.to_vec());
    let names = level_names(&all);
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
            let name = e.get(164..180).map(|n| String::from_utf8_lossy(n).trim_end_matches('\0').trim().to_ascii_uppercase()).unwrap_or_default();
            let mut j = i + 1;
            while j < all.len() && all[j][0] & 0x80 != 0 {
                j += 1;
            }
            defs.insert(name, read_run(&all[i + 1..j], &colors));
            i = j;
            continue;
        }
        top.push(e);
        i += 1;
    }
    let raw = read_run(&top, &colors);
    let per_unit = u.uor_per_sub * u.sub_per_master;
    let m: Affine = [1.0 / per_unit, 0.0, 0.0, 1.0 / per_unit, -u.origin[0] / per_unit, -u.origin[1] / per_unit];
    let mut sheet = Sheet { sub_per_master: u.sub_per_master, table_colors, ..Default::default() };
    place(&raw, &m, &defs, 0, &mut sheet);
    sheet.rect = paths_bounds(&sheet.paths).or_else(|| {
        let t = sheet.texts.first()?;
        Some([t.origin[0], t.origin[1], t.origin[0] + t.height, t.origin[1] + t.height])
    })?;
    Some(sheet)
}
