// Compiled AutoCAD shape-file (.SHX) reader + shape tessellator.
//
// Parses the classic `AutoCAD-86 shapes 1.0/1.1` container (the format SHAPE
// entities and complex-linetype shapes reference) and interprets a shape's
// bytecode into unit-space polylines. Fonts (`AutoCAD-86 unifont` etc.) are
// out of scope here — text keeps going through the LFF/TTF substitutes.
//
// Bytecode reference (each item, pen starts DOWN at the origin):
//   0            end of shape
//   1 / 2        pen down / pen up
//   3 n / 4 n    divide / multiply the vector scale by n
//   5 / 6        push / pop the current position (4-deep stack)
//   7 n          draw subshape n
//   8 dx dy      signed byte displacement
//   9 …          repeated signed (dx,dy) pairs until (0,0)
//   0xA r f      octant arc: radius, flags (sign|start-octant|octant-count)
//   0xB s e hr r f  fractional arc
//   0xC dx dy b  bulge segment; 0xD … repeated until (0,0)
//   0xE          vertical-text-only flag: skip the NEXT item horizontally
//   else         vector: high nibble = length, low nibble = one of 16
//                predefined directions
//
// Results are cached per (file, shape-number); parse tables per file.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// One shape's tessellation in unit space: polylines of XY points.
pub type ShapePolylines = Arc<Vec<Vec<[f64; 2]>>>;

/// The 16 SHX vector directions. Diagonal-ish entries deliberately use the
/// classic half-steps (not normalized) — lengths scale per direction exactly
/// as AutoCAD draws them.
const DIRS: [[f64; 2]; 16] = [
    [1.0, 0.0],
    [1.0, 0.5],
    [1.0, 1.0],
    [0.5, 1.0],
    [0.0, 1.0],
    [-0.5, 1.0],
    [-1.0, 1.0],
    [-1.0, 0.5],
    [-1.0, 0.0],
    [-1.0, -0.5],
    [-1.0, -1.0],
    [-0.5, -1.0],
    [0.0, -1.0],
    [0.5, -1.0],
    [1.0, -1.0],
    [1.0, -0.5],
];

struct ShxFile {
    /// shape number → (name, spec bytes)
    shapes: HashMap<u16, (String, Vec<u8>)>,
}

fn file_cache() -> &'static Mutex<HashMap<String, Option<Arc<ShxFile>>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<Arc<ShxFile>>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn shape_cache() -> &'static Mutex<HashMap<(String, u16), Option<(ShapePolylines, f64)>>> {
    static C: OnceLock<Mutex<HashMap<(String, u16), Option<(ShapePolylines, f64)>>>> =
        OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_file(path: &str) -> Option<Arc<ShxFile>> {
    if let Ok(c) = file_cache().lock() {
        if let Some(hit) = c.get(path) {
            return hit.clone();
        }
    }
    let parsed = parse_file(path).map(Arc::new);
    if let Ok(mut c) = file_cache().lock() {
        c.insert(path.to_string(), parsed.clone());
    }
    parsed
}

fn parse_file(path: &str) -> Option<ShxFile> {
    let bytes = std::fs::read(path).ok()?;
    // Header line: `AutoCAD-86 shapes 1.0`/`1.1` + CR LF SUB. Reject fonts.
    let head_end = bytes.iter().position(|&b| b == 0x1A)?;
    let header = String::from_utf8_lossy(&bytes[..head_end]);
    if !header.contains("shapes") {
        return None;
    }
    let mut p = head_end + 1;
    let rd_u16 = |p: &mut usize| -> Option<u16> {
        let v = u16::from_le_bytes([*bytes.get(*p)?, *bytes.get(*p + 1)?]);
        *p += 2;
        Some(v)
    };
    let _first = rd_u16(&mut p)?;
    let _last = rd_u16(&mut p)?;
    let count = rd_u16(&mut p)? as usize;
    let mut dir: Vec<(u16, usize)> = Vec::with_capacity(count);
    for _ in 0..count {
        let num = rd_u16(&mut p)?;
        let len = rd_u16(&mut p)? as usize;
        dir.push((num, len));
    }
    let mut shapes = HashMap::new();
    for (num, len) in dir {
        let end = p.checked_add(len)?.min(bytes.len());
        let blob = &bytes[p..end];
        p = end;
        // Name = leading NUL-terminated ASCII, spec follows.
        let name_end = blob.iter().position(|&b| b == 0).unwrap_or(0);
        let name = String::from_utf8_lossy(&blob[..name_end]).into_owned();
        let spec = blob.get(name_end + 1..).unwrap_or(&[]).to_vec();
        shapes.insert(num, (name, spec));
    }
    Some(ShxFile { shapes })
}

/// Tessellate the shape named `name` (case-insensitive) from the SHX at
/// `path` — the `.lin` catalog references linetype shapes by name.
pub fn shape_polylines_by_name(path: &str, name: &str) -> Option<ShapePolylines> {
    let file = load_file(path)?;
    let num = file
        .shapes
        .iter()
        .find(|(_, (n, _))| n.eq_ignore_ascii_case(name))
        .map(|(&num, _)| num)?;
    shape_polylines(path, num)
}

/// Tessellate `shape_number` from the SHX at `path` into unit-space
/// polylines. `None` when the file/shape is missing or not a shapes file.
pub fn shape_polylines(path: &str, shape_number: u16) -> Option<ShapePolylines> {
    shape_with_advance(path, shape_number).map(|(p, _)| p)
}

/// Like [`shape_polylines`], plus the pen's final X — a font glyph's advance.
pub fn shape_with_advance(path: &str, shape_number: u16) -> Option<(ShapePolylines, f64)> {
    let key = (path.to_string(), shape_number);
    if let Ok(c) = shape_cache().lock() {
        if let Some(hit) = c.get(&key) {
            return hit.clone();
        }
    }
    let built = build_shape(path, shape_number);
    if let Ok(mut c) = shape_cache().lock() {
        c.insert(key, built.clone());
    }
    built
}

fn build_shape(path: &str, shape_number: u16) -> Option<(ShapePolylines, f64)> {
    let file = load_file(path)?;
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut cur: Vec<[f64; 2]> = vec![[0.0, 0.0]];
    let mut st = InterpState {
        x: 0.0,
        y: 0.0,
        scale: 1.0,
        pen_down: true,
        stack: Vec::new(),
    };
    interpret(&file, shape_number, &mut st, &mut cur, &mut out, 0);
    if cur.len() > 1 {
        out.push(cur);
    }
    let advance = st.x;
    if out.is_empty() {
        // A pen-up-only glyph (space) still carries its advance.
        if advance.abs() < 1e-12 {
            return None;
        }
        return Some((Arc::new(Vec::new()), advance));
    }
    Some((Arc::new(out), advance))
}

// ── SHX text fonts ────────────────────────────────────────────────────────────
//
// A classic SHX *font* is the same container with glyphs keyed by character
// code and shape #0 holding the font header: `above, below, modes, 0` (the
// cap height above the baseline and the descender depth).

/// Font metrics: (above, below). `None` when the file has no shape #0 header.
pub fn font_metrics(path: &str) -> Option<(f64, f64)> {
    let file = load_file(path)?;
    let (_, spec) = file.shapes.get(&0)?;
    let above = *spec.first()? as f64;
    let below = *spec.get(1)? as f64;
    if above <= 0.0 {
        return None;
    }
    Some((above, below))
}

fn glyph_cache(
) -> &'static Mutex<HashMap<(String, u16), Option<Arc<crate::scene::text::lff::Glyph>>>> {
    static C: OnceLock<Mutex<HashMap<(String, u16), Option<Arc<crate::scene::text::lff::Glyph>>>>> =
        OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Look up character `code` as a font glyph, normalised so the font's
/// `above` (cap height) maps to 9 units — the scale every stroke-font
/// consumer in the text pipeline expects.
pub fn font_glyph(path: &str, code: u16) -> Option<Arc<crate::scene::text::lff::Glyph>> {
    let key = (path.to_string(), code);
    if let Ok(c) = glyph_cache().lock() {
        if let Some(hit) = c.get(&key) {
            return hit.clone();
        }
    }
    let built = (|| {
        let (above, _) = font_metrics(path)?;
        let (polys, advance) = shape_with_advance(path, code)?;
        let k = 9.0 / above;
        let strokes: Vec<Vec<[f32; 2]>> = polys
            .iter()
            .filter(|p| p.len() > 1)
            .map(|p| {
                p.iter()
                    .map(|&[x, y]| [(x * k) as f32, (y * k) as f32])
                    .collect()
            })
            .collect();
        Some(Arc::new(crate::scene::text::lff::Glyph {
            strokes,
            advance: (advance * k) as f32,
            fill_tris: Vec::new(),
        }))
    })();
    if let Ok(mut c) = glyph_cache().lock() {
        c.insert(key, built.clone());
    }
    built
}

struct InterpState {
    x: f64,
    y: f64,
    scale: f64,
    pen_down: bool,
    stack: Vec<(f64, f64)>,
}

/// Append a straight segment / move in interpreter space.
fn advance(
    st: &mut InterpState,
    cur: &mut Vec<[f64; 2]>,
    out: &mut Vec<Vec<[f64; 2]>>,
    dx: f64,
    dy: f64,
) {
    st.x += dx * st.scale;
    st.y += dy * st.scale;
    if st.pen_down {
        cur.push([st.x, st.y]);
    } else {
        if cur.len() > 1 {
            out.push(std::mem::take(cur));
        }
        cur.clear();
        cur.push([st.x, st.y]);
    }
}

/// Arc from the current point: `start`/`sweep` in radians over `r` (already
/// scale-applied). Sampled into short segments.
fn emit_arc(
    st: &mut InterpState,
    cur: &mut Vec<[f64; 2]>,
    out: &mut Vec<Vec<[f64; 2]>>,
    r: f64,
    start: f64,
    sweep: f64,
) {
    if r <= 0.0 || sweep == 0.0 {
        return;
    }
    let cx = st.x - r * start.cos();
    let cy = st.y - r * start.sin();
    let steps = ((sweep.abs() / std::f64::consts::TAU * 32.0).ceil() as usize).max(2);
    for i in 1..=steps {
        let a = start + sweep * i as f64 / steps as f64;
        let nx = cx + r * a.cos();
        let ny = cy + r * a.sin();
        if st.pen_down {
            cur.push([nx, ny]);
        } else if i == steps {
            if cur.len() > 1 {
                out.push(std::mem::take(cur));
            }
            cur.clear();
            cur.push([nx, ny]);
        }
        st.x = nx;
        st.y = ny;
    }
}

/// Bulge segment (like LWPOLYLINE bulge, in ±127 units).
fn emit_bulge(
    st: &mut InterpState,
    cur: &mut Vec<[f64; 2]>,
    out: &mut Vec<Vec<[f64; 2]>>,
    dx: f64,
    dy: f64,
    bulge: f64,
) {
    let (sx, sy) = (st.x, st.y);
    let (ex, ey) = (sx + dx * st.scale, sy + dy * st.scale);
    let b = bulge / 127.0;
    if b.abs() < 1e-6 || !st.pen_down {
        advance(st, cur, out, dx, dy);
        return;
    }
    // Bulge = tan(theta/4); sample the arc through the chord.
    let theta = 4.0 * b.atan();
    let chord = ((ex - sx).powi(2) + (ey - sy).powi(2)).sqrt();
    if chord < 1e-12 {
        return;
    }
    let r = chord / (2.0 * (theta / 2.0).sin().abs());
    let mid_x = (sx + ex) * 0.5;
    let mid_y = (sy + ey) * 0.5;
    let d = (r * r - chord * chord / 4.0).max(0.0).sqrt() * if theta > 0.0 { 1.0 } else { -1.0 };
    let (nx, ny) = (-(ey - sy) / chord, (ex - sx) / chord);
    let (cx, cy) = (mid_x + nx * d, mid_y + ny * d);
    let a0 = (sy - cy).atan2(sx - cx);
    let steps = ((theta.abs() / std::f64::consts::TAU * 32.0).ceil() as usize).max(2);
    for i in 1..=steps {
        let a = a0 + theta * i as f64 / steps as f64;
        let px = cx + r * a.cos();
        let py = cy + r * a.sin();
        cur.push([px, py]);
        st.x = px;
        st.y = py;
    }
    st.x = ex;
    st.y = ey;
    if let Some(last) = cur.last_mut() {
        *last = [ex, ey];
    }
}

fn interpret(
    file: &ShxFile,
    shape_number: u16,
    st: &mut InterpState,
    cur: &mut Vec<[f64; 2]>,
    out: &mut Vec<Vec<[f64; 2]>>,
    depth: usize,
) {
    if depth > 4 {
        return;
    }
    let Some((_, spec)) = file.shapes.get(&shape_number) else {
        return;
    };
    let b = spec.as_slice();
    let mut i = 0usize;
    let mut skip_next = false;
    while i < b.len() {
        let code = b[i];
        i += 1;
        // 0xE: the next item applies to vertical text only — decode it to
        // know its length, but discard its effect.
        let skipping = skip_next;
        skip_next = false;
        match code {
            0 => return,
            1 => {
                if !skipping {
                    st.pen_down = true;
                }
            }
            2 => {
                if !skipping {
                    st.pen_down = false;
                }
            }
            3 | 4 => {
                let n = *b.get(i).unwrap_or(&1) as f64;
                i += 1;
                if !skipping && n > 0.0 {
                    if code == 3 {
                        st.scale /= n;
                    } else {
                        st.scale *= n;
                    }
                }
            }
            5 => {
                if !skipping && st.stack.len() < 4 {
                    st.stack.push((st.x, st.y));
                }
            }
            6 => {
                if !skipping {
                    if let Some((px, py)) = st.stack.pop() {
                        st.x = px;
                        st.y = py;
                        if cur.len() > 1 {
                            out.push(std::mem::take(cur));
                        }
                        cur.clear();
                        cur.push([px, py]);
                    }
                }
            }
            7 => {
                let sub = *b.get(i).unwrap_or(&0) as u16;
                i += 1;
                if !skipping && sub != 0 && sub != shape_number {
                    interpret(file, sub, st, cur, out, depth + 1);
                }
            }
            8 => {
                let dx = *b.get(i).unwrap_or(&0) as i8 as f64;
                let dy = *b.get(i + 1).unwrap_or(&0) as i8 as f64;
                i += 2;
                if !skipping {
                    advance(st, cur, out, dx, dy);
                }
            }
            9 => loop {
                let dx = *b.get(i).unwrap_or(&0) as i8 as f64;
                let dy = *b.get(i + 1).unwrap_or(&0) as i8 as f64;
                i += 2;
                if dx == 0.0 && dy == 0.0 {
                    break;
                }
                if !skipping {
                    advance(st, cur, out, dx, dy);
                }
            },
            0xA => {
                let r = *b.get(i).unwrap_or(&0) as f64;
                let f = *b.get(i + 1).unwrap_or(&0);
                i += 2;
                if !skipping {
                    let cw = f & 0x80 != 0;
                    let start_oct = ((f >> 4) & 7) as f64;
                    let mut n_oct = (f & 7) as f64;
                    if n_oct == 0.0 {
                        n_oct = 8.0;
                    }
                    let start = start_oct * std::f64::consts::FRAC_PI_4;
                    let sweep = n_oct * std::f64::consts::FRAC_PI_4 * if cw { -1.0 } else { 1.0 };
                    emit_arc(st, cur, out, r * st.scale, start, sweep);
                }
            }
            0xB => {
                let so = *b.get(i).unwrap_or(&0) as f64;
                let eo = *b.get(i + 1).unwrap_or(&0) as f64;
                let hr = *b.get(i + 2).unwrap_or(&0) as f64;
                let r = *b.get(i + 3).unwrap_or(&0) as f64;
                let f = *b.get(i + 4).unwrap_or(&0);
                i += 5;
                if !skipping {
                    let radius = (hr * 256.0 + r) * st.scale;
                    let cw = f & 0x80 != 0;
                    let start_oct = ((f >> 4) & 7) as f64;
                    let mut n_oct = (f & 7) as f64;
                    if n_oct == 0.0 {
                        n_oct = 8.0;
                    }
                    let q = std::f64::consts::FRAC_PI_4;
                    let start = start_oct * q + so * q / 256.0 * if cw { -1.0 } else { 1.0 };
                    let total = n_oct * q;
                    let sweep_mag = total - so * q / 256.0 - (255.0 - eo) * q / 256.0
                        + if eo == 0.0 { 0.0 } else { 0.0 };
                    let sweep = sweep_mag.max(0.0) * if cw { -1.0 } else { 1.0 };
                    emit_arc(st, cur, out, radius, start, sweep);
                }
            }
            0xC => {
                let dx = *b.get(i).unwrap_or(&0) as i8 as f64;
                let dy = *b.get(i + 1).unwrap_or(&0) as i8 as f64;
                let bu = *b.get(i + 2).unwrap_or(&0) as i8 as f64;
                i += 3;
                if !skipping {
                    emit_bulge(st, cur, out, dx, dy, bu);
                }
            }
            0xD => loop {
                let dx = *b.get(i).unwrap_or(&0) as i8 as f64;
                let dy = *b.get(i + 1).unwrap_or(&0) as i8 as f64;
                i += 2;
                if dx == 0.0 && dy == 0.0 {
                    break;
                }
                let bu = *b.get(i).unwrap_or(&0) as i8 as f64;
                i += 1;
                if !skipping {
                    emit_bulge(st, cur, out, dx, dy, bu);
                }
            },
            0xE => {
                skip_next = true;
            }
            v => {
                let len = (v >> 4) as f64;
                let dir = DIRS[(v & 0xF) as usize];
                if !skipping {
                    advance(st, cur, out, dir[0] * len, dir[1] * len);
                }
            }
        }
    }
}
