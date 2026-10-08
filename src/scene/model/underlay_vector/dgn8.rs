//! DGN V8 design files: an OLE compound file whose models (Dgn-Md/#n)
//! keep their graphic elements in a compressed stream (Dgn^G/$1), and whose
//! non-model stream (Dgn^Nm/$1) holds the colour table, the levels and the
//! shared cell definitions. Element fields are little-endian; coordinates
//! are UORs.

use std::collections::HashMap;
use std::io::Read;

use super::model::{arc_cubics, dgn_color_table, paths_bounds, Path, PathBuilder, Segment, Sheet, SubPath, Text, DGN_DEFAULT_COLORS};

// ── Compound file ───────────────────────────────────────────────────────────

struct Cfb<'a> {
    b: &'a [u8],
    sector: usize,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    mini: Vec<u8>,
    cutoff: u32,
    entries: Vec<Entry>,
}

struct Entry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u32,
}

const END: u32 = 0xFFFF_FFFA;

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl<'a> Cfb<'a> {
    fn open(b: &'a [u8]) -> Option<Self> {
        // The 512-byte header, with the only two sector sizes the format
        // has (512 and 4096 bytes); anything else is not a compound file.
        if b.len() < 512 || !b.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
            return None;
        }
        let sector_shift = u16::from_le_bytes([b[30], b[31]]);
        if sector_shift != 9 && sector_shift != 12 {
            return None;
        }
        let sector = 1usize << sector_shift;
        let mini_sector_shift = u16::from_le_bytes([b[32], b[33]]);
        if mini_sector_shift != 6 {
            return None;
        }
        let mut difat: Vec<u32> = (0..109).filter_map(|i| u32_at(b, 76 + i * 4)).filter(|&v| v < END).collect();
        // The DIFAT chain cannot hold more sectors than the file does, so a
        // looping chain stops there.
        let (mut next, mut count) = (u32_at(b, 68)?, u32_at(b, 72)?.min((b.len() / sector) as u32));
        while count > 0 && next < END {
            let at = (next as usize + 1) * sector;
            for i in 0..sector / 4 - 1 {
                if let Some(v) = u32_at(b, at + i * 4).filter(|&v| v < END) {
                    difat.push(v);
                }
            }
            next = u32_at(b, at + sector - 4)?;
            count -= 1;
        }
        let mut fat = Vec::new();
        for s in difat {
            let at = (s as usize + 1) * sector;
            for i in 0..sector / 4 {
                fat.push(u32_at(b, at + i * 4).unwrap_or(END));
            }
        }
        let mut cfb = Cfb { b, sector, fat, mini_fat: Vec::new(), mini: Vec::new(), cutoff: u32_at(b, 56)?, entries: Vec::new() };
        let dir = cfb.chain(u32_at(b, 48)?, None);
        for e in dir.chunks_exact(128) {
            let len = u16::from_le_bytes([e[64], e[65]]) as usize;
            let name16: Vec<u16> = e[..len.saturating_sub(2).min(64)].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            cfb.entries.push(Entry {
                name: String::from_utf16_lossy(&name16),
                kind: e[66],
                left: u32_at(e, 68)?,
                right: u32_at(e, 72)?,
                child: u32_at(e, 76)?,
                start: u32_at(e, 116)?,
                size: u32_at(e, 120)?,
            });
        }
        let root = cfb.entries.first()?;
        let (root_start, root_size) = (root.start, root.size);
        cfb.mini = cfb.chain(root_start, Some(root_size as usize));
        let mini_fat = cfb.chain(u32_at(b, 60)?, None);
        cfb.mini_fat = mini_fat.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        Some(cfb)
    }

    fn chain(&self, mut s: u32, size: Option<usize>) -> Vec<u8> {
        // A FAT entry can point at itself (`fat[s] = s`): the old fixed
        // 1_000_000-iteration guard still let every call accumulate
        // `1e6 × sector` bytes (4.1 GB at sector_shift = 12) — directory,
        // mini stream and miniFAT chains are alive at once (~12 GB peak →
        // allocation-failure abort). A chain can only read in-file sectors,
        // and after that many steps it must have repeated one; the walk is
        // deterministic, so a repeat means a cycle that yields nothing new.
        // Bounding by the file's own sector count never truncates a
        // legitimate chain (each sector read at most once).
        let max_sectors = self.b.len() / self.sector;
        let mut out = Vec::new();
        let mut guard = 0;
        while s < END && guard < max_sectors {
            let at = (s as usize + 1) * self.sector;
            let Some(block) = self.b.get(at..at + self.sector) else { break };
            out.extend_from_slice(block);
            s = self.fat.get(s as usize).copied().unwrap_or(END);
            guard += 1;
        }
        if let Some(n) = size {
            out.truncate(n);
        }
        out
    }

    fn stream(&self, e: &Entry) -> Vec<u8> {
        if e.size >= self.cutoff {
            return self.chain(e.start, Some(e.size as usize));
        }
        let mut out = Vec::new();
        let mut s = e.start;
        let mut guard = 0;
        // Same cycle argument as `chain`, over the mini sectors: a loop in
        // the miniFAT used to spin the full 1_000_000-iteration budget
        // (64 MB per call) before `truncate` shortened the length.
        let max_sectors = self.mini.len() / 64;
        while s < END && guard < max_sectors {
            let at = s as usize * 64;
            let Some(block) = self.mini.get(at..at + 64) else { break };
            out.extend_from_slice(block);
            s = self.mini_fat.get(s as usize).copied().unwrap_or(END);
            guard += 1;
        }
        out.truncate(e.size as usize);
        out
    }

    /// Every stream with its full path ("Dgn-Md/#000000/Dgn^G/$1").
    fn streams(&self) -> Vec<(String, &Entry)> {
        let mut out = Vec::new();
        fn walk<'b>(c: &'b Cfb, idx: u32, path: &str, out: &mut Vec<(String, &'b Entry)>, depth: usize) {
            let Some(e) = c.entries.get(idx as usize) else { return };
            if depth > 64 {
                return;
            }
            walk(c, e.left, path, out, depth + 1);
            let p = if path.is_empty() { e.name.clone() } else { format!("{path}/{}", e.name) };
            if e.kind == 2 {
                out.push((p.clone(), e));
            } else if e.kind == 1 {
                walk(c, e.child, &p, out, depth + 1);
            }
            walk(c, e.right, path, out, depth + 1);
        }
        if let Some(root) = self.entries.first() {
            walk(self, root.child, "", &mut out, 0);
        }
        out
    }
}

/// A DGN stream: raw, or zlib data after a 16-byte header.
fn inflate(data: &[u8]) -> Vec<u8> {
    // Far above any real element stream; stops a crafted one from
    // inflating until memory runs out.
    const STREAM_LIMIT: u64 = 512 << 20;
    for skip in [16usize, 0] {
        if let Some(z) = data.get(skip..) {
            let mut out = Vec::new();
            if flate2::read::ZlibDecoder::new(z).take(STREAM_LIMIT).read_to_end(&mut out).is_ok()
                && !out.is_empty()
            {
                return out;
            }
        }
    }
    data.to_vec()
}

// ── Elements ────────────────────────────────────────────────────────────────

/// Element records of an element stream (after its 4-byte count).
fn elements(d: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 4;
    while at + 8 <= d.len() {
        let Some(words) = u32_at(d, at + 4) else { break };
        let len = 4 + words as usize * 2;
        if len < 8 {
            break;
        }
        // The stream can end inside its last element (its trailing words are
        // not stored); what is there is still read.
        let e = &d[at..(at + len).min(d.len())];
        if !is_deleted(e) {
            out.push(e);
        }
        at += len;
    }
    out
}

fn kind(e: &[u8]) -> u8 {
    e[0]
}

fn flags(e: &[u8]) -> u16 {
    u16::from_le_bytes([e[2], e[3]])
}

/// A deleted element (flag 0x80 of its second byte), kept in the stream but
/// no longer part of the file.
fn is_deleted(e: &[u8]) -> bool {
    e[1] & 0x80 != 0
}

/// A component of a complex element, cell or shared cell definition.
fn is_component(e: &[u8]) -> bool {
    flags(e) & 0x4000 != 0
}

fn f64_at(e: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(e.get(at..at + 8)?.try_into().ok()?))
}


/// A string stored after the "ff fe 01 00" marker, when the element has one.
fn marked_string(e: &[u8], from: usize) -> Option<String> {
    let i = e.get(from..)?.windows(4).position(|w| w == [0xff, 0xfe, 0x01, 0x00])? + from + 4;
    let end = e[i..].iter().position(|&c| c == 0 || c < 0x20).map(|n| i + n).unwrap_or(e.len());
    Some(String::from_utf8_lossy(&e[i..end]).to_string())
}

struct Styles {
    colors: Vec<[u8; 3]>,
    /// Level id → colour index.
    levels: HashMap<u32, u32>,
    /// Levels the underlay turns off: their elements are left out.
    hidden: std::collections::HashSet<u32>,
    /// True colours, in the order colour words number them (1 first).
    extended: Vec<[u8; 3]>,
}

/// The true colours of the file: a zlib-packed UTF-16 record in the
/// non-model attributes stream reading
/// `<ExtendedColors><Entry Color="(r,g,b)"/>…</ExtendedColors>`.
fn extended_colors(cfb: &Cfb) -> Vec<[u8; 3]> {
    let Some(attrs) = cfb.streams().iter().find(|(p, _)| p == "Dgn^NmA/$1").map(|(_, e)| inflate(&cfb.stream(e))) else {
        return Vec::new();
    };
    for at in 0..attrs.len().saturating_sub(2) {
        // A zlib header: deflate, and a check value divisible by 31.
        let (a, b) = (attrs[at], attrs[at + 1]);
        if a & 0x0f != 8 || ((a as u16) << 8 | b as u16) % 31 != 0 {
            continue;
        }
        // The record is a short XML list; the cap keeps a stream that
        // inflates without end (or a crafted one) from exhausting memory.
        const RECORD_LIMIT: u64 = 4 << 20;
        let mut out = Vec::new();
        if flate2::read::ZlibDecoder::new(&attrs[at..])
            .take(RECORD_LIMIT)
            .read_to_end(&mut out)
            .is_err()
            || out.len() < 2
        {
            continue;
        }
        let units: Vec<u16> = out.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let text = String::from_utf16_lossy(&units);
        if !text.contains("<ExtendedColors") {
            continue;
        }
        return text
            .split("Color=\"(")
            .skip(1)
            .filter_map(|rest| {
                let v: Vec<u8> = rest.split(')').next()?.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                (v.len() == 3).then(|| [v[0], v[1], v[2]])
            })
            .collect();
    }
    Vec::new()
}

impl Styles {
    fn color(&self, e: &[u8]) -> [u8; 3] {
        // The colour word: an index into the colour table, 0xFFFFFFFF for
        // the level's colour, or true colour n (from 1) × 256 plus the index
        // of its nearest table colour, drawn in its own RGB when the file
        // lists it.
        let explicit = u32_at(e, 52).unwrap_or(u32::MAX);
        let index = if explicit == u32::MAX {
            self.levels.get(&u32_at(e, 12).unwrap_or(0)).copied().unwrap_or(0)
        } else {
            if let Some(rgb) = (explicit >> 8).checked_sub(1).and_then(|n| self.extended.get(n as usize)) {
                return *rgb;
            }
            explicit & 0xff
        };
        self.colors.get(index as usize).copied().unwrap_or([255, 255, 255])
    }
}

/// Plan geometry in UORs, and the shared cells it places.
#[derive(Default, Clone)]
struct Raw {
    paths: Vec<Path>,
    texts: Vec<Text>,
    instances: Vec<Instance>,
}

#[derive(Clone)]
struct Instance {
    name: String,
    /// The definition's placement, with every enclosing one applied.
    xf: Xf,
}

/// A placement in UORs: x' = r[0]·(x, y, z, 1), y' = r[1]·…, z' = r[2]·….
type Xf = [[f64; 4]; 3];

const IDENTITY: Xf = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

fn xf_vector(m: &Xf, v: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}

fn xf_point(m: &Xf, p: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3])
}

fn xf_compose(outer: &Xf, inner: &Xf) -> Xf {
    let mut out = [[0.0; 4]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| outer[i][k] * inner[k][j]).sum::<f64>() + if j == 3 { outer[i][3] } else { 0.0 };
        }
    }
    out
}

/// A 3D element (flag 0x0800 of its properties word): three coordinates a
/// point, orientations as quaternions. Its plan view is drawn.
fn is_3d(e: &[u8]) -> bool {
    u32_at(e, 40).is_some_and(|v| v & 0x0800 != 0)
}

/// The in-plane x and y axes of a 3D element's orientation quaternion w, x,
/// y, z stored at `at` (the rows of its rotation matrix).
fn quat_axes(e: &[u8], at: usize) -> Option<([f64; 3], [f64; 3])> {
    let (w, x, y, z) = (f64_at(e, at)?, f64_at(e, at + 8)?, f64_at(e, at + 16)?, f64_at(e, at + 24)?);
    Some((
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
    ))
}

/// The in-plane axes of a 2D element turned by `rotation` (radians).
fn turned_axes(rotation: f64) -> ([f64; 3], [f64; 3]) {
    let (c, s) = (rotation.cos(), rotation.sin());
    ([c, s, 0.0], [-s, c, 0.0])
}

/// An elliptical arc on axes `u` and `v` (plan projections) as cubics.
fn projected_arc(c: [f64; 2], a: f64, b: f64, u: [f64; 2], v: [f64; 2], start: f64, sweep: f64) -> Vec<Segment> {
    let map = |p: [f64; 2]| [c[0] + a * p[0] * u[0] + b * p[1] * v[0], c[1] + a * p[0] * u[1] + b * p[1] * v[1]];
    arc_cubics([0.0, 0.0], 1.0, 1.0, 0.0, start, sweep)
        .into_iter()
        .map(|[p0, p1, p2, p3]| Segment::Cubic(map(p0), map(p1), map(p2), map(p3)))
        .collect()
}

/// Reads a run of elements placed through `xf` into plan geometry.
fn read_run(run: &[&[u8]], styles: &Styles, xf: &Xf) -> Raw {
    let mut raw = Raw::default();
    // A complex shape or chain collects its components into one path.
    let mut complex: Option<(usize, PathBuilder, Option<([u8; 3], f64)>, Option<[u8; 3]>, bool)> = None;
    fn push(raw: &mut Raw, complex: &mut Option<(usize, PathBuilder, Option<([u8; 3], f64)>, Option<[u8; 3]>, bool)>, path: Path) {
        if let Some((left, pb, _, _, _)) = complex.as_mut() {
            for sp in &path.subpaths {
                for seg in &sp.segments {
                    let (a, b) = match seg {
                        Segment::Line(a, b) | Segment::Cubic(a, _, _, b) => (*a, *b),
                    };
                    if pb.current() != Some(a) {
                        if pb.current().is_none() { pb.move_to(a) } else { pb.line_to(a) }
                    }
                    match seg {
                        Segment::Line(..) => pb.line_to(b),
                        Segment::Cubic(_, c1, c2, _) => pb.cubic_to(*c1, *c2, b),
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
    }
    let plan = |p: [f64; 3]| {
        let q = xf_point(xf, p);
        [q[0], q[1]]
    };
    let axis = |v: [f64; 3]| {
        let q = xf_vector(xf, v);
        [q[0], q[1]]
    };
    for e in run {
        if e.len() < 104 || styles.hidden.contains(&u32_at(e, 12).unwrap_or(0)) {
            continue;
        }
        let color = styles.color(e);
        let stroke = Some((color, -1.0));
        let d3 = is_3d(e);
        // A point: x, y (and z in a 3D element).
        let p3 = |at: usize| -> Option<[f64; 3]> {
            Some([f64_at(e, at)?, f64_at(e, at + 8)?, if d3 { f64_at(e, at + 16)? } else { 0.0 }])
        };
        match kind(e) {
            12 | 14 => {
                let n = u32_at(e, 104).unwrap_or(0) as usize;
                if n > 0 {
                    let fill = (kind(e) == 14 && e.windows(4).any(|w| w == [0x07, 0x10, 0x41, 0x00])).then_some(color);
                    complex = Some((n, PathBuilder::default(), stroke, fill, kind(e) == 14));
                }
            }
            3 => {
                if let (Some(a), Some(b)) = (p3(104), p3(if d3 { 128 } else { 120 })) {
                    push(&mut raw, &mut complex, Path { subpaths: vec![SubPath { segments: vec![Segment::Line(plan(a), plan(b))], closed: false }], stroke, fill: None });
                }
            }
            4 | 6 | 11 => {
                let n = u32_at(e, 104).unwrap_or(0) as usize;
                let stride = if d3 { 24 } else { 16 };
                let pts: Vec<[f64; 2]> = (0..n).filter_map(|k| p3(112 + k * stride)).map(plan).collect();
                if pts.len() >= 2 {
                    let mut pb = PathBuilder::default();
                    pb.move_to(pts[0]);
                    for p in &pts[1..] {
                        pb.line_to(*p);
                    }
                    if kind(e) == 6 {
                        pb.close();
                    }
                    push(&mut raw, &mut complex, Path { subpaths: pb.finish(), stroke, fill: None });
                }
            }
            15 | 16 => {
                // Radii, orientation and centre. 3D: ellipse a 104, b 112,
                // quaternion 120, centre 152; arc start 104, sweep 112, a 120,
                // b 128, quaternion 136, centre 168. 2D: the orientation is an
                // angle, ellipse centre 128, arc centre 144.
                let arc = match (kind(e), d3) {
                    (15, true) => (|| Some((f64_at(e, 104)?, f64_at(e, 112)?, quat_axes(e, 120)?, p3(152)?, 0.0, 0.0)))(),
                    (_, true) => (|| Some((f64_at(e, 120)?, f64_at(e, 128)?, quat_axes(e, 136)?, p3(168)?, f64_at(e, 104)?, f64_at(e, 112)?)))(),
                    (15, false) => (|| Some((f64_at(e, 104)?, f64_at(e, 112)?, turned_axes(f64_at(e, 120)?), p3(128)?, 0.0, 0.0)))(),
                    _ => (|| Some((f64_at(e, 120)?, f64_at(e, 128)?, turned_axes(f64_at(e, 136)?), p3(144)?, f64_at(e, 104)?, f64_at(e, 112)?)))(),
                };
                let Some((a, b, (u, v), c, start, sweep)) = arc else { continue };
                let sweep = if sweep == 0.0 { std::f64::consts::TAU } else { sweep };
                let closed = sweep.abs() >= std::f64::consts::TAU - 1e-9;
                push(
                    &mut raw,
                    &mut complex,
                    Path { subpaths: vec![SubPath { segments: projected_arc(plan(c), a, b, axis(u), axis(v), start, sweep), closed }], stroke, fill: None },
                );
            }
            17 => {
                // 3D: width 112, height 120, quaternion 144, origin 176, the
                // characters after the string marker. 2D: angle 144, origin
                // 152, a byte count (with its 4-byte marker) at 110 and the
                // characters from 174.
                let placed = if d3 {
                    (|| Some((f64_at(e, 112)?, f64_at(e, 120)?, quat_axes(e, 144)?, p3(176)?, marked_string(e, 200)?)))()
                } else {
                    (|| {
                        let count = u16::from_le_bytes(e.get(110..112)?.try_into().ok()?) as usize;
                        let chars = e.get(174..170 + count.max(4))?;
                        let text = String::from_utf8_lossy(chars).trim_end_matches(char::from(0)).to_string();
                        Some((f64_at(e, 112)?, f64_at(e, 120)?, turned_axes(f64_at(e, 144)?), p3(152)?, text))
                    })()
                };
                let Some((width, height, (u, v), origin, text)) = placed else { continue };
                // The text's axes as the plan shows them: its baseline sets the
                // angle, and each axis's plan length scales its size.
                let (pu, pv) = (axis(u), axis(v));
                let (su, sv) = (pu[0].hypot(pu[1]), pv[0].hypot(pv[1]));
                if su < 1e-9 || sv < 1e-9 {
                    continue;
                }
                let (height, width) = (height * 6.0 / 1000.0 * sv, width * 6.0 / 1000.0 * su);
                raw.texts.push(Text {
                    text,
                    origin: plan(origin),
                    height,
                    width_factor: if height > 0.0 { width / height } else { 1.0 },
                    rotation: pu[1].atan2(pu[0]),
                    color,
                    font: "txt".to_string(),
                });
            }
            35 => {
                // Shared cell instance: a 3×3 matrix by rows from 160, the
                // origin at 232 (z at 248 in 3D), the definition's name after
                // the string marker.
                let m: Option<Vec<f64>> = (0..9).map(|k| f64_at(e, 160 + k * 8)).collect();
                let (Some(m), Some(o)) = (m, p3(232)) else { continue };
                let Some(name) = marked_string(e, 248) else { continue };
                let local: Xf = [[m[0], m[1], m[2], o[0]], [m[3], m[4], m[5], o[1]], [m[6], m[7], m[8], o[2]]];
                raw.instances.push(Instance { name: name.to_ascii_uppercase(), xf: xf_compose(xf, &local) });
            }
            _ => {}
        }
    }
    // A complex element cut short keeps the components it has.
    if let Some((_, mut pb, stroke, fill, closed)) = complex.take() {
        if closed {
            pb.close();
        }
        raw.paths.push(Path { subpaths: pb.finish(), stroke, fill });
    }
    raw
}

type Affine = [f64; 6];

fn apply(m: &Affine, p: [f64; 2]) -> [f64; 2] {
    [m[0] * p[0] + m[1] * p[1] + m[4], m[2] * p[0] + m[3] * p[1] + m[5]]
}

/// Places plan geometry through the sheet mapping `m`, and the shared cells
/// it instances (each definition read through its own placement).
fn place(raw: &Raw, m: &Affine, defs: &HashMap<String, Vec<&[u8]>>, styles: &Styles, depth: usize, out: &mut Sheet) {
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
        let Some(def) = defs.get(&inst.name) else { continue };
        place(&read_run(def, styles, &inst.xf), m, defs, styles, depth + 1, out);
    }
}

/// Units of a model header: UORs per master and per sub unit.
fn units(header: &[u8]) -> (f64, f64) {
    // Unit definitions are numerator/denominator pairs per metre: master
    // (4180/4188), sub (4196/4204), storage (4332/4340), and the UORs per
    // storage unit (4324).
    let get = |at: usize| f64_at(header, at).filter(|v| v.is_finite() && *v > 0.0);
    let (mn, md, sn, sd) = (get(4180), get(4188), get(4196), get(4204));
    let (uor, stn, std) = (get(4324), get(4332), get(4340));
    let uor_per_metre = match (uor, stn, std) {
        (Some(u), Some(n), Some(d)) => u * n / d,
        _ => 1_000_000.0,
    };
    let master = match (mn, md) {
        (Some(n), Some(d)) => uor_per_metre * d / n,
        _ => uor_per_metre,
    };
    let sub = match (sn, sd) {
        (Some(n), Some(d)) => uor_per_metre * d / n,
        _ => master / 1000.0,
    };
    (master, sub)
}


/// Models of the file: (name, graphic stream, header stream).
fn models(cfb: &Cfb) -> Vec<(String, Vec<u8>, Vec<u8>)> {
    let streams = cfb.streams();
    let mut dirs: Vec<String> = streams
        .iter()
        .filter_map(|(p, _)| p.strip_suffix("/Dgn^G/$1").map(|d| d.to_string()))
        .collect();
    dirs.sort();
    // Each stream may inflate to `STREAM_LIMIT` (512 MB) and every model
    // dir inflates its own — entries can all point at the same payload, so
    // N dirs × 512 MB was reachable from one small stream. The budget
    // scales with the file (×64 covers any real compression ratio, so
    // legitimate files never trip it) with a 256 MiB floor for small ones;
    // models past the budget are dropped, the earlier ones still load.
    let total_limit = cfb.b.len().saturating_mul(64).max(256 << 20);
    let mut total = 0usize;
    let mut out = Vec::new();
    for dir in dirs {
        if total >= total_limit {
            break;
        }
        let get = |suffix: &str| streams.iter().find(|(p, _)| *p == format!("{dir}/{suffix}")).map(|(_, e)| inflate(&cfb.stream(e))).unwrap_or_default();
        let graphics = get("Dgn^G/$1");
        let header = get("Dgn~Mh");
        total = total
            .saturating_add(graphics.len())
            .saturating_add(header.len());
        let name = marked_string(&header, 0).unwrap_or_else(|| "Default".to_string());
        out.push((name, graphics, header));
    }
    out
}

pub fn model_names(bytes: &[u8]) -> Option<Vec<String>> {
    let cfb = Cfb::open(bytes)?;
    let names: Vec<String> = models(&cfb).into_iter().map(|m| m.0).collect();
    (!names.is_empty()).then_some(names)
}

/// The level table (the table entries after a header of table 1): id,
/// name and colour index. The default level (64) is listed as "0", as the
/// reference names it.
fn level_table(nm: &[&[u8]]) -> Vec<(u32, String, u32)> {
    let mut out = Vec::new();
    let mut in_levels = false;
    for e in nm {
        match kind(e) {
            96 => in_levels = u32_at(e, 12) == Some(1),
            95 if in_levels && e.len() >= 80 => {
                let (Some(id), Some(color)) = (u32_at(e, 32), u32_at(e, 72)) else { continue };
                let mut name = marked_string(e, 32).unwrap_or_default();
                if id == 64 && name.eq_ignore_ascii_case("Default") {
                    name = "0".to_string();
                }
                out.push((id, name, color));
            }
            _ => {}
        }
    }
    out
}

fn non_model(cfb: &Cfb) -> Vec<u8> {
    cfb.streams().iter().find(|(p, _)| p == "Dgn^Nm/$1").map(|(_, e)| inflate(&cfb.stream(e))).unwrap_or_default()
}

/// Level names of the file, for the underlay layers list.
pub fn layer_names(bytes: &[u8]) -> Option<Vec<String>> {
    let cfb = Cfb::open(bytes)?;
    let nm = non_model(&cfb);
    Some(level_table(&elements(&nm)).into_iter().map(|l| l.1).filter(|n| !n.is_empty()).collect())
}

pub fn model(bytes: &[u8], name: &str, hidden: &[String]) -> Option<Sheet> {
    let cfb = Cfb::open(bytes)?;
    let all = models(&cfb);
    let (_, graphics, header) = all.iter().find(|m| m.0.eq_ignore_ascii_case(name)).or_else(|| all.first())?;
    let non_model = non_model(&cfb);
    let nm = elements(&non_model);
    // Colours 0-254 from byte 37, the background (255) opening the table.
    let color_table = nm.iter().find(|e| kind(e) == 5).and_then(|e| dgn_color_table(e, 37));
    let table_colors = color_table.is_some();
    let colors = color_table.unwrap_or_else(|| DGN_DEFAULT_COLORS.to_vec());
    let table = level_table(&nm);
    let levels = table.iter().map(|l| (l.0, l.2)).collect();
    let hidden = table
        .iter()
        .filter(|l| hidden.iter().any(|h| h.eq_ignore_ascii_case(&l.1)))
        .map(|l| l.0)
        .collect();
    let styles = Styles { colors, levels, hidden, extended: extended_colors(&cfb) };
    // Shared cell definitions: a type-34 element and its components.
    let mut defs: HashMap<String, Vec<&[u8]>> = HashMap::new();
    let mut i = 0;
    while i < nm.len() {
        if kind(nm[i]) == 34 && !is_component(nm[i]) {
            let name = marked_string(nm[i], 248).unwrap_or_default().to_ascii_uppercase();
            let mut j = i + 1;
            while j < nm.len() && is_component(nm[j]) {
                j += 1;
            }
            defs.insert(name, nm[i + 1..j].to_vec());
            i = j;
        } else {
            i += 1;
        }
    }
    let graphics = elements(graphics);
    let raw = read_run(&graphics, &styles, &IDENTITY);
    let (per_unit, sub_uor) = units(header);
    let origin = global_origin(header);
    let m: Affine = [1.0 / per_unit, 0.0, 0.0, 1.0 / per_unit, -origin[0] / per_unit, -origin[1] / per_unit];
    let mut sheet = Sheet { sub_per_master: per_unit / sub_uor, table_colors, ..Default::default() };
    place(&raw, &m, &defs, &styles, 0, &mut sheet);
    // The model's extent is its elements' stored ranges (text by its full
    // box), as the reference sizes a model.
    sheet.rect = match stored_range(&graphics) {
        Some([x0, y0, x1, y1]) => {
            let (lo, hi) = (apply(&m, [x0, y0]), apply(&m, [x1, y1]));
            [lo[0], lo[1], hi[0], hi[1]]
        }
        None => paths_bounds(&sheet.paths)?,
    };
    Some(sheet)
}

/// The model's global origin (UORs; the design plane point drawn at the
/// model's 0,0).
fn global_origin(header: &[u8]) -> [f64; 2] {
    let get = |at: usize| f64_at(header, at).filter(|v| v.is_finite()).unwrap_or(0.0);
    [get(4212), get(4220)]
}

/// The union of the top-level elements' ranges (UORs): each graphic element
/// stores its low corner at 56 and its size at 80, as 64-bit integers.
fn stored_range(graphics: &[&[u8]]) -> Option<[f64; 4]> {
    let i64_at = |e: &[u8], at: usize| Some(i64::from_le_bytes(e.get(at..at + 8)?.try_into().ok()?) as f64);
    let mut r = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for e in graphics.iter().filter(|e| e.len() >= 104 && !is_component(e)) {
        let (Some(x), Some(y), Some(w), Some(h)) = (i64_at(e, 56), i64_at(e, 64), i64_at(e, 80), i64_at(e, 88)) else { continue };
        r = [r[0].min(x), r[1].min(y), r[2].max(x + w), r[3].max(y + h)];
    }
    (r[0] <= r[2]).then_some(r)
}

#[cfg(test)]
mod cfb_memory_tests {
    use super::*;

    const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
    const NONE: u32 = 0xFFFF_FFFF;

    /// Directory entries start at the first sector — `(0 + 1) * sector`,
    /// so the header occupies a full sector, not just its first 512 bytes.
    fn put_entry(
        b: &mut [u8],
        base: usize,
        idx: usize,
        name: &str,
        kind: u8,
        left: u32,
        right: u32,
        child: u32,
        start: u32,
        size: u32,
    ) {
        let e = base + idx * 128;
        let utf16: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        for (i, ch) in utf16.iter().enumerate() {
            b[e + i * 2..e + i * 2 + 2].copy_from_slice(&ch.to_le_bytes());
        }
        b[e + 64..e + 66].copy_from_slice(&((utf16.len() * 2) as u16).to_le_bytes());
        b[e + 66] = kind;
        b[e + 68..e + 72].copy_from_slice(&left.to_le_bytes());
        b[e + 72..e + 76].copy_from_slice(&right.to_le_bytes());
        b[e + 76..e + 80].copy_from_slice(&child.to_le_bytes());
        b[e + 116..e + 120].copy_from_slice(&start.to_le_bytes());
        b[e + 120..e + 124].copy_from_slice(&size.to_le_bytes());
    }

    fn header(b: &mut [u8], sector_shift: u16, dir_start: u32, minifat_start: u32, difat0: u32) {
        b[0..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
        b[30..32].copy_from_slice(&sector_shift.to_le_bytes());
        b[32..34].copy_from_slice(&6u16.to_le_bytes()); // mini sector shift
        b[48..52].copy_from_slice(&dir_start.to_le_bytes());
        b[56..60].copy_from_slice(&4096u32.to_le_bytes()); // mini cutoff
        b[60..64].copy_from_slice(&minifat_start.to_le_bytes());
        b[68..72].copy_from_slice(&ENDOFCHAIN.to_le_bytes()); // DIFAT chain: none
        b[72..76].copy_from_slice(&0u32.to_le_bytes()); // DIFAT chain count: 0
        b[76..80].copy_from_slice(&difat0.to_le_bytes()); // header DIFAT slot 0
        for slot in 1..109 {
            b[76 + slot * 4..80 + slot * 4].copy_from_slice(&NONE.to_le_bytes());
        }
    }

    /// ~1.5 KB CFB (sector_shift = 9): directory and miniFAT both start at
    /// sector 0, whose FAT entry (sector 1, all zeros) points at itself —
    /// `chain()` loops its full 1_000_000-iteration budget → 512 MB per
    /// call, three calls alive at once.
    fn hostile_self_fat_cfb() -> Vec<u8> {
        let mut b = vec![0u8; 3 * 512];
        header(&mut b, 9, 0, 0, 1);
        // Sector 0 = directory: entry 0 is the root; its start field also
        // points at sector 0 (self-loop), size = u32::MAX so the mini chain
        // is never truncated down to a small length.
        put_entry(&mut b, 512, 0, "Root", 1, NONE, NONE, NONE, 0, u32::MAX);
        // Sector 1 = FAT, already zero: fat[0] = 0 (self-referential).
        b
    }

    /// Write the model storages' left/right sibling links as a balanced
    /// binary tree over entry indices `2, 4, …, 2·n` (walk depth O(log n)).
    fn link_models(b: &mut [u8], base: usize, model_count: usize) {
        fn build(b: &mut [u8], base: usize, lo: usize, hi: usize) -> u32 {
            if lo > hi {
                return NONE;
            }
            let mid = (lo + hi) / 2;
            let left = if mid > lo { build(b, base, lo, mid - 1) } else { NONE };
            let right = if mid < hi { build(b, base, mid + 1, hi) } else { NONE };
            let e = base + (2 + mid * 2) * 128;
            b[e + 68..e + 72].copy_from_slice(&left.to_le_bytes());
            b[e + 72..e + 76].copy_from_slice(&right.to_le_bytes());
            (2 + mid * 2) as u32
        }
        build(b, base, 0, model_count - 1);
    }

    /// 300 model directories all sharing one 4 MB stream: `models()`
    /// retains every model simultaneously with no total budget → 1.2 GB
    /// from a 4 MB file (inflate() falls back to a raw copy of the zeros).
    fn multi_model_cfb() -> Vec<u8> {
        let sector = 4096usize;
        let model_count = 300usize;
        let entry_count = 2 + model_count * 2;
        let dir_sectors = (entry_count * 128 + sector - 1) / sector;
        let payload_len = 4 << 20;
        let payload_sectors = payload_len / sector;
        // Sectors: 0..dir_sectors directory; then 2 FAT; then payload.
        let fat_start = dir_sectors;
        let payload_start = fat_start + 2;
        let mut b = vec![0u8; (payload_start + payload_sectors + 1) * sector];
        header(&mut b, 12, 0, ENDOFCHAIN, fat_start as u32);
        b[80..84].copy_from_slice(&((fat_start + 1) as u32).to_le_bytes()); // DIFAT slot 1
        // Directory: root → storage → model storages, sibling links as a
        // balanced tree (real CFB directories are red-black; a linear chain
        // would hit streams()' existing depth-64 guard). Each storage has
        // one "Dgn^G/$1" stream pointing at the shared payload.
        let root_model = (model_count - 1) / 2;
        put_entry(&mut b, sector, 0, "Root", 1, NONE, NONE, 1, ENDOFCHAIN, 0);
        put_entry(
            &mut b,
            sector,
            1,
            "Dgn-Md",
            1,
            NONE,
            NONE,
            (2 + root_model * 2) as u32,
            ENDOFCHAIN,
            0,
        );
        for i in 0..model_count {
            let model = 2 + i * 2;
            let stream = model + 1;
            put_entry(
                &mut b,
                sector,
                model,
                &format!("Model{i}"),
                1,
                NONE,
                NONE,
                stream as u32,
                ENDOFCHAIN,
                0,
            );
            put_entry(
                &mut b,
                sector,
                stream,
                "Dgn^G/$1",
                2,
                NONE,
                NONE,
                NONE,
                payload_start as u32,
                payload_len as u32,
            );
        }
        link_models(&mut b, sector, model_count);
        // Directory chain across its sectors.
        let fat_at = (fat_start + 1) * sector;
        for i in 0..dir_sectors {
            let at = fat_at + i * 4;
            let next = if i + 1 < dir_sectors {
                (i + 1) as u32
            } else {
                ENDOFCHAIN
            };
            b[at..at + 4].copy_from_slice(&next.to_le_bytes());
        }
        // Both FAT sectors are not part of any chain; payload chain follows.
        for i in 0..2 {
            let at = fat_at + (fat_start + i) * 4;
            b[at..at + 4].copy_from_slice(&ENDOFCHAIN.to_le_bytes());
        }
        for i in 0..payload_sectors {
            let at = fat_at + (payload_start + i) * 4;
            let next = if i + 1 < payload_sectors {
                (payload_start + i + 1) as u32
            } else {
                ENDOFCHAIN
            };
            b[at..at + 4].copy_from_slice(&next.to_le_bytes());
        }
        b
    }

    /// A self-referential FAT must not let `chain()` (directory, mini
    /// stream, miniFAT) grow past the file it was read from: the old fixed
    /// 1_000_000-iteration guard built 512 MB per call from 1.5 KB.
    #[test]
    fn a_self_referential_fat_builds_no_more_bytes_than_the_file() {
        let bytes = hostile_self_fat_cfb();
        let cfb = Cfb::open(&bytes).expect("hostile CFB parses");
        assert!(
            cfb.entries.len() * 128 <= bytes.len(),
            "directory grew to {} entries from a {} byte file",
            cfb.entries.len(),
            bytes.len()
        );
        assert!(
            cfb.mini.len() <= bytes.len(),
            "mini stream grew to {} bytes from a {} byte file",
            cfb.mini.len(),
            bytes.len()
        );
        assert!(
            cfb.mini_fat.len() * 4 <= bytes.len(),
            "miniFAT grew to {} bytes from a {} byte file",
            cfb.mini_fat.len() * 4,
            bytes.len()
        );
    }

    /// Hundreds of model dirs sharing one stream: `models()` inflates all
    /// of them at once. The total must stay within the file-scaled budget,
    /// while the first models still load.
    #[test]
    fn models_keep_their_total_inflated_size_within_budget() {
        let bytes = multi_model_cfb();
        let cfb = Cfb::open(&bytes).expect("multi-model CFB parses");
        let out = models(&cfb);
        let total: usize = out.iter().map(|(_, g, h)| g.len() + h.len()).sum();
        let budget = (bytes.len() * 64).max(256 << 20);
        assert!(
            total <= budget + (512 << 20),
            "models inflated to {} bytes from a {} byte file (budget {budget})",
            total,
            bytes.len()
        );
        assert!(!out.is_empty(), "the first models still load");
        assert!(out.len() < 300, "models past the budget are dropped");
        assert!(model_names(&bytes).is_some());
    }
}
