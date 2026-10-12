// EMF (enhanced metafile) record player.
//
// Walks the record stream and replays the drawing subset onto a `Canvas`
// through the `Dc` transform stack. Unknown records are skipped by their
// self-declared size, so partially supported files still draw everything
// this player understands.

use super::raster::{Canvas, Rgba};
use super::{canvas_for_box, colorref, dib, stock_obj, text, Brush, Dc, Font, Obj, Pen, Xform};

#[inline]
fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
#[inline]
fn i32_at(d: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
#[inline]
fn f32_at(d: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
#[inline]
fn i16_at(d: &[u8], o: usize) -> i16 {
    i16::from_le_bytes(d[o..o + 2].try_into().unwrap())
}

fn xform_at(d: &[u8], o: usize) -> Xform {
    Xform {
        m11: f32_at(d, o),
        m12: f32_at(d, o + 4),
        m21: f32_at(d, o + 8),
        m22: f32_at(d, o + 12),
        dx: f32_at(d, o + 16),
        dy: f32_at(d, o + 20),
    }
}

/// Flatten a cubic Bézier run (`pts[0]` current point, then control triples).
fn flatten_beziers(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    const STEPS: usize = 12;
    let mut out = Vec::new();
    if pts.is_empty() {
        return out;
    }
    out.push(pts[0]);
    let mut p0 = pts[0];
    let mut i = 1;
    while i + 2 < pts.len() {
        let (c1, c2, p3) = (pts[i], pts[i + 1], pts[i + 2]);
        for k in 1..=STEPS {
            let t = k as f32 / STEPS as f32;
            let u = 1.0 - t;
            out.push([
                u * u * u * p0[0]
                    + 3.0 * u * u * t * c1[0]
                    + 3.0 * u * t * t * c2[0]
                    + t * t * t * p3[0],
                u * u * u * p0[1]
                    + 3.0 * u * u * t * c1[1]
                    + 3.0 * u * t * t * c2[1]
                    + t * t * t * p3[1],
            ]);
        }
        p0 = p3;
        i += 3;
    }
    out
}

struct Player {
    dc: Dc,
    objects: Vec<Option<Obj>>,
    /// BEGINPATH..ENDPATH contour accumulator (logical coords).
    path: Option<Vec<Vec<[f32; 2]>>>,
}

impl Player {
    fn select(&mut self, ih: u32) {
        let obj = if ih & 0x8000_0000 != 0 {
            stock_obj(ih & 0x7FFF_FFFF)
        } else {
            self.objects.get(ih as usize).and_then(|o| o.clone())
        };
        match obj {
            Some(Obj::Pen(p)) => self.dc.pen = p,
            Some(Obj::Brush(b)) => self.dc.brush = b,
            Some(Obj::Font(f)) => self.dc.font = f,
            _ => {}
        }
    }

    fn store(&mut self, ih: u32, obj: Obj) {
        let i = ih as usize;
        if i >= self.objects.len() {
            if i > 4096 {
                return; // corrupt index — don't balloon the table
            }
            self.objects.resize(i + 1, None);
        }
        self.objects[i] = Some(obj);
    }

    /// Convert logical points to canvas space.
    fn map(&self, pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
        pts.iter().map(|p| self.dc.to_canvas(p[0], p[1])).collect()
    }

    fn fill_and_stroke(&mut self, canvas: &mut Canvas, rings: &[Vec<[f32; 2]>], close: bool) {
        let mapped: Vec<Vec<[f32; 2]>> = rings.iter().map(|r| self.map(r)).collect();
        if !self.dc.brush.null && close {
            canvas.fill_polys(&mapped, self.dc.brush.color);
        }
        if !self.dc.pen.null {
            let w = self.dc.pen_px();
            for r in &mapped {
                if close && r.len() >= 2 {
                    let mut closed = r.clone();
                    closed.push(r[0]);
                    canvas.stroke_polyline(&closed, w, self.dc.pen.color);
                } else {
                    canvas.stroke_polyline(r, w, self.dc.pen.color);
                }
            }
        }
    }

    /// Points of a 16-bit poly record at `off`, `n` entries.
    fn pts16(rec: &[u8], off: usize, n: usize) -> Vec<[f32; 2]> {
        // `n` is a raw u32 out of the record — for POLYPOLYLINE it is the
        // per-ring count, which the `total > 400_000` check never bounds:
        // `u32::MAX` used to request a ~17 GB capacity up front and abort
        // on allocation failure. Only bytes present can be read (loop
        // below), so the capacity is sized from the record.
        let avail = rec.len().saturating_sub(off) / 4;
        let mut v = Vec::with_capacity(n.min(avail));
        for i in 0..n {
            let o = off + i * 4;
            if o + 4 > rec.len() {
                break;
            }
            v.push([i16_at(rec, o) as f32, i16_at(rec, o + 2) as f32]);
        }
        v
    }

    fn pts32(rec: &[u8], off: usize, n: usize) -> Vec<[f32; 2]> {
        let avail = rec.len().saturating_sub(off) / 8;
        let mut v = Vec::with_capacity(n.min(avail));
        for i in 0..n {
            let o = off + i * 8;
            if o + 8 > rec.len() {
                break;
            }
            v.push([i32_at(rec, o) as f32, i32_at(rec, o + 4) as f32]);
        }
        v
    }
}

pub fn render(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    if data.len() < 88 || u32_at(data, 0) != 1 || &data[40..44] != b" EMF" {
        return None;
    }
    // Header bounds: the device-space box the picture occupies.
    let (bl, bt, br, bb) = (
        i32_at(data, 8) as f32,
        i32_at(data, 12) as f32,
        i32_at(data, 16) as f32,
        i32_at(data, 20) as f32,
    );
    let (mut canvas, dev_scale) = canvas_for_box(br - bl, bb - bt)?;
    let mut p = Player {
        dc: Dc::new(),
        objects: Vec::new(),
        path: None,
    };
    p.dc.dev_org = (bl, bt);
    p.dc.dev_scale = dev_scale;

    let mut off = 0usize;
    let mut guard = 0usize;
    while off + 8 <= data.len() {
        guard += 1;
        if guard > 4_000_000 {
            break;
        }
        let ty = u32_at(data, off);
        let size = u32_at(data, off + 4) as usize;
        if size < 8 || size % 4 != 0 || off + size > data.len() {
            break;
        }
        let rec = &data[off..off + size];
        off += size;
        match ty {
            14 => break, // EOF
            9 if rec.len() >= 16 => {
                let (cx, cy) = (i32_at(rec, 8) as f32, i32_at(rec, 12) as f32);
                if cx != 0.0 && cy != 0.0 {
                    p.dc.win_ext = (cx, cy);
                }
            }
            10 if rec.len() >= 16 => p.dc.win_org = (i32_at(rec, 8) as f32, i32_at(rec, 12) as f32),
            11 if rec.len() >= 16 => {
                let (cx, cy) = (i32_at(rec, 8) as f32, i32_at(rec, 12) as f32);
                if cx != 0.0 && cy != 0.0 {
                    p.dc.vp_ext = (cx, cy);
                }
            }
            12 if rec.len() >= 16 => p.dc.vp_org = (i32_at(rec, 8) as f32, i32_at(rec, 12) as f32),
            17 if rec.len() >= 12 => {
                // MM_TEXT resets to the identity logical→device mapping.
                if u32_at(rec, 8) == 1 {
                    p.dc.win_org = (0.0, 0.0);
                    p.dc.win_ext = (1.0, 1.0);
                    p.dc.vp_org = (0.0, 0.0);
                    p.dc.vp_ext = (1.0, 1.0);
                }
            }
            18 if rec.len() >= 12 => p.dc.bk_mode = u32_at(rec, 8),
            22 if rec.len() >= 12 => p.dc.text_align = u32_at(rec, 8),
            24 if rec.len() >= 12 => p.dc.text_color = colorref(u32_at(rec, 8)),
            25 if rec.len() >= 12 => p.dc.bk_color = colorref(u32_at(rec, 8)),
            27 if rec.len() >= 16 => {
                p.dc.pos = (i32_at(rec, 8) as f32, i32_at(rec, 12) as f32);
                if let Some(path) = &mut p.path {
                    path.push(vec![[p.dc.pos.0, p.dc.pos.1]]);
                }
            }
            30 if rec.len() >= 24 => {
                let a =
                    p.dc.to_canvas(i32_at(rec, 8) as f32, i32_at(rec, 12) as f32);
                let b =
                    p.dc.to_canvas(i32_at(rec, 16) as f32, i32_at(rec, 20) as f32);
                let c = &mut canvas.clip;
                c.0 = c.0.max(a[0].min(b[0]));
                c.1 = c.1.max(a[1].min(b[1]));
                c.2 = c.2.min(a[0].max(b[0]));
                c.3 = c.3.min(a[1].max(b[1]));
            }
            33 => p.dc.save(&canvas),
            34 => {
                let n = if rec.len() >= 12 {
                    i32_at(rec, 8).unsigned_abs().max(1)
                } else {
                    1
                };
                for _ in 0..n {
                    p.dc.restore(&mut canvas);
                }
            }
            35 if rec.len() >= 32 => p.dc.world = xform_at(rec, 8),
            36 if rec.len() >= 36 => {
                let x = xform_at(rec, 8);
                match u32_at(rec, 32) {
                    1 => p.dc.world = Xform::IDENTITY,
                    2 => p.dc.world = p.dc.world.mul(&x),
                    3 => p.dc.world = x.mul(&p.dc.world),
                    _ => p.dc.world = x,
                }
            }
            37 if rec.len() >= 12 => p.select(u32_at(rec, 8)),
            38 if rec.len() >= 28 => {
                let style = u32_at(rec, 12);
                p.store(
                    u32_at(rec, 8),
                    Obj::Pen(Pen {
                        color: colorref(u32_at(rec, 24)),
                        width: i32_at(rec, 16) as f32,
                        null: style & 0xF == 5,
                    }),
                );
            }
            39 if rec.len() >= 24 => {
                let style = u32_at(rec, 12);
                p.store(
                    u32_at(rec, 8),
                    Obj::Brush(Brush {
                        color: colorref(u32_at(rec, 16)),
                        null: style == 1,
                    }),
                );
            }
            40 if rec.len() >= 12 => {
                let ih = u32_at(rec, 8) as usize;
                if ih < p.objects.len() {
                    p.objects[ih] = None;
                }
            }
            42 | 43 if rec.len() >= 24 => {
                let (l, t, r, b) = (
                    i32_at(rec, 8) as f32,
                    i32_at(rec, 12) as f32,
                    i32_at(rec, 16) as f32,
                    i32_at(rec, 20) as f32,
                );
                let ring = if ty == 43 {
                    vec![[l, t], [r, t], [r, b], [l, b]]
                } else {
                    let (cx, cy) = ((l + r) * 0.5, (t + b) * 0.5);
                    let (rx, ry) = ((r - l) * 0.5, (b - t) * 0.5);
                    (0..64)
                        .map(|i| {
                            let a = i as f32 / 64.0 * std::f32::consts::TAU;
                            [cx + rx * a.cos(), cy + ry * a.sin()]
                        })
                        .collect()
                };
                p.fill_and_stroke(&mut canvas, std::slice::from_ref(&ring), true);
            }
            44 if rec.len() >= 24 => {
                // ROUNDRECT — drawn square-cornered.
                let (l, t, r, b) = (
                    i32_at(rec, 8) as f32,
                    i32_at(rec, 12) as f32,
                    i32_at(rec, 16) as f32,
                    i32_at(rec, 20) as f32,
                );
                let ring = vec![[l, t], [r, t], [r, b], [l, b]];
                p.fill_and_stroke(&mut canvas, std::slice::from_ref(&ring), true);
            }
            54 if rec.len() >= 16 => {
                let to = [i32_at(rec, 8) as f32, i32_at(rec, 12) as f32];
                if let Some(path) = &mut p.path {
                    match path.last_mut() {
                        Some(c) => c.push(to),
                        None => path.push(vec![[p.dc.pos.0, p.dc.pos.1], to]),
                    }
                } else if !p.dc.pen.null {
                    let seg = p.map(&[[p.dc.pos.0, p.dc.pos.1], to]);
                    let w = p.dc.pen_px();
                    canvas.stroke_polyline(&seg, w, p.dc.pen.color);
                }
                p.dc.pos = (to[0], to[1]);
            }
            59 => p.path = Some(Vec::new()),
            60 => {}
            61 => {
                if let Some(path) = &mut p.path {
                    if let Some(c) = path.last_mut() {
                        if let Some(&first) = c.first() {
                            c.push(first);
                        }
                    }
                }
            }
            62 | 63 | 64 => {
                if let Some(path) = p.path.take() {
                    let close = ty != 64;
                    let stroke = ty != 62;
                    let saved_pen = p.dc.pen;
                    let saved_brush = p.dc.brush;
                    if !stroke {
                        p.dc.pen.null = true;
                    }
                    if ty == 64 {
                        p.dc.brush.null = true;
                    }
                    p.fill_and_stroke(&mut canvas, &path, close);
                    p.dc.pen = saved_pen;
                    p.dc.brush = saved_brush;
                }
            }
            76 | 77 if rec.len() >= 100 => {
                let (xd, yd, cxd, cyd) = (
                    i32_at(rec, 24) as f32,
                    i32_at(rec, 28) as f32,
                    i32_at(rec, 32) as f32,
                    i32_at(rec, 36) as f32,
                );
                let rop = u32_at(rec, 40);
                let cb_bmi = u32_at(rec, 88) as usize;
                let cb_bits = u32_at(rec, 96) as usize;
                if cb_bits > 0 && cb_bmi > 0 {
                    let off_bmi = u32_at(rec, 84) as usize;
                    let off_bits = u32_at(rec, 92) as usize;
                    if let Some((bmi, bits)) = dib::ranges(rec, off_bmi, cb_bmi, off_bits, cb_bits)
                    {
                        if let Some((px, w, h)) = dib::decode(bmi, bits) {
                            let (sx, sy) = (i32_at(rec, 44) as f32, i32_at(rec, 48) as f32);
                            let (scx, scy) = if ty == 77 && rec.len() >= 108 {
                                (i32_at(rec, 100) as f32, i32_at(rec, 104) as f32)
                            } else {
                                (cxd, cyd)
                            };
                            let a = p.dc.to_canvas(xd, yd);
                            let b = p.dc.to_canvas(xd + cxd, yd + cyd);
                            canvas.blit(
                                &px, w as usize, h as usize, sx, sy, scx, scy, a[0], a[1], b[0],
                                b[1],
                            );
                        }
                    }
                } else {
                    let a = p.dc.to_canvas(xd, yd);
                    let b = p.dc.to_canvas(xd + cxd, yd + cyd);
                    let color = match rop {
                        0x00F0_0021 | 0x005A_0049 => Some(p.dc.brush.color), // PATCOPY / PATINVERT
                        0x0000_0042 => Some(Rgba([0, 0, 0, 255])),           // BLACKNESS
                        0x00FF_0062 => Some(Rgba([255, 255, 255, 255])),     // WHITENESS
                        _ => None,
                    };
                    if let Some(c) = color {
                        if !p.dc.brush.null || !matches!(rop, 0x00F0_0021 | 0x005A_0049) {
                            canvas.fill_rect(
                                a[0].min(b[0]),
                                a[1].min(b[1]),
                                a[0].max(b[0]),
                                a[1].max(b[1]),
                                c,
                            );
                        }
                    }
                }
            }
            80 if rec.len() >= 76 => {
                let (xd, yd) = (i32_at(rec, 24) as f32, i32_at(rec, 28) as f32);
                let (sx, sy, scx, scy) = (
                    i32_at(rec, 32) as f32,
                    i32_at(rec, 36) as f32,
                    i32_at(rec, 40) as f32,
                    i32_at(rec, 44) as f32,
                );
                let (off_bmi, cb_bmi, off_bits, cb_bits) = (
                    u32_at(rec, 48) as usize,
                    u32_at(rec, 52) as usize,
                    u32_at(rec, 56) as usize,
                    u32_at(rec, 60) as usize,
                );
                if let Some((bmi, bits)) = dib::ranges(rec, off_bmi, cb_bmi, off_bits, cb_bits) {
                    if let Some((px, w, h)) = dib::decode(bmi, bits) {
                        let a = p.dc.to_canvas(xd, yd);
                        let b = p.dc.to_canvas(xd + scx, yd + scy);
                        canvas.blit(
                            &px, w as usize, h as usize, sx, sy, scx, scy, a[0], a[1], b[0], b[1],
                        );
                    }
                }
            }
            81 if rec.len() >= 80 => {
                let (xd, yd) = (i32_at(rec, 24) as f32, i32_at(rec, 28) as f32);
                let (sx, sy, scx, scy) = (
                    i32_at(rec, 32) as f32,
                    i32_at(rec, 36) as f32,
                    i32_at(rec, 40) as f32,
                    i32_at(rec, 44) as f32,
                );
                let (off_bmi, cb_bmi, off_bits, cb_bits) = (
                    u32_at(rec, 48) as usize,
                    u32_at(rec, 52) as usize,
                    u32_at(rec, 56) as usize,
                    u32_at(rec, 60) as usize,
                );
                let (cxd, cyd) = (i32_at(rec, 72) as f32, i32_at(rec, 76) as f32);
                if let Some((bmi, bits)) = dib::ranges(rec, off_bmi, cb_bmi, off_bits, cb_bits) {
                    if let Some((px, w, h)) = dib::decode(bmi, bits) {
                        let a = p.dc.to_canvas(xd, yd);
                        let b = p.dc.to_canvas(xd + cxd, yd + cyd);
                        canvas.blit(
                            &px, w as usize, h as usize, sx, sy, scx, scy, a[0], a[1], b[0], b[1],
                        );
                    }
                }
            }
            82 if rec.len() >= 104 => {
                let mut name = String::new();
                for i in 0..32 {
                    let o = 40 + i * 2;
                    let c = u16::from_le_bytes(rec[o..o + 2].try_into().unwrap());
                    if c == 0 {
                        break;
                    }
                    name.push(char::from_u32(c as u32).unwrap_or('?'));
                }
                p.store(
                    u32_at(rec, 8),
                    Obj::Font(Font {
                        height: i32_at(rec, 12) as f32,
                        escapement: i32_at(rec, 20) as f32,
                        facename: name,
                        charset: rec[35], // lfCharSet: LOGFONT offset 23 → record 35
                    }),
                );
            }
            83 | 84 if rec.len() >= 76 => {
                let is_wide = ty == 84;
                let (rx, ry) = (i32_at(rec, 36) as f32, i32_at(rec, 40) as f32);
                let n = u32_at(rec, 44) as usize;
                let off_str = u32_at(rec, 48) as usize;
                let opts = u32_at(rec, 52);
                let rcl = [
                    i32_at(rec, 56) as f32,
                    i32_at(rec, 60) as f32,
                    i32_at(rec, 64) as f32,
                    i32_at(rec, 68) as f32,
                ];
                let off_dx = u32_at(rec, 72) as usize;
                if n == 0 || n > 4096 {
                    continue;
                }
                let s = if is_wide {
                    let end = off_str + n * 2;
                    if end > rec.len() {
                        continue;
                    }
                    let units: Vec<u16> = (0..n)
                        .map(|i| {
                            u16::from_le_bytes(
                                rec[off_str + i * 2..off_str + i * 2 + 2]
                                    .try_into()
                                    .unwrap(),
                            )
                        })
                        .collect();
                    String::from_utf16_lossy(&units)
                } else {
                    let end = off_str + n;
                    if end > rec.len() {
                        continue;
                    }
                    text::decode_ansi(&rec[off_str..end], &p.dc.font)
                };
                let dx: Option<Vec<f32>> = if off_dx != 0 && off_dx + n * 4 <= rec.len() {
                    Some((0..n).map(|i| u32_at(rec, off_dx + i * 4) as f32).collect())
                } else {
                    None
                };
                let opaque = if opts & 2 != 0 { Some(rcl) } else { None };
                text::draw_text(&mut canvas, &mut p.dc, rx, ry, &s, dx.as_deref(), opaque);
            }
            2..=8 | 85..=91 if rec.len() >= 28 => {
                let wide = ty <= 8;
                let (base, kind) = if wide { (0, ty) } else { (0, ty - 83) };
                let _ = base;
                match kind {
                    // POLYBEZIER(2)/POLYGON(3)/POLYLINE(4)/POLYBEZIERTO(5)/POLYLINETO(6)
                    2..=6 => {
                        let n = u32_at(rec, 24) as usize;
                        if n == 0 || n > 200_000 {
                            continue;
                        }
                        let mut pts = if wide {
                            Player::pts32(rec, 28, n)
                        } else {
                            Player::pts16(rec, 28, n)
                        };
                        let to_variant = kind == 5 || kind == 6;
                        if to_variant {
                            pts.insert(0, [p.dc.pos.0, p.dc.pos.1]);
                        }
                        if let Some(&last) = pts.last() {
                            if to_variant {
                                p.dc.pos = (last[0], last[1]);
                            }
                        }
                        let flat = if kind == 2 || kind == 5 {
                            flatten_beziers(&pts)
                        } else {
                            pts
                        };
                        if let Some(path) = &mut p.path {
                            if to_variant {
                                match path.last_mut() {
                                    Some(c) => c.extend_from_slice(&flat[1..]),
                                    None => path.push(flat),
                                }
                            } else {
                                path.push(flat);
                            }
                        } else if kind == 3 {
                            p.fill_and_stroke(&mut canvas, std::slice::from_ref(&flat), true);
                        } else if !p.dc.pen.null {
                            let mapped = p.map(&flat);
                            let w = p.dc.pen_px();
                            canvas.stroke_polyline(&mapped, w, p.dc.pen.color);
                        }
                    }
                    // POLYPOLYLINE(7)/POLYPOLYGON(8)
                    7 | 8 => {
                        // The shared arm guard only promises 28 bytes, but
                        // this sub-arm reads `total` at 28 and starts the
                        // counts array at 32: `nSize = 28` records used to
                        // panic `range end index 32 out of len 28`.
                        if rec.len() < 32 {
                            continue;
                        }
                        let n_polys = u32_at(rec, 24) as usize;
                        let total = u32_at(rec, 28) as usize;
                        if n_polys == 0 || n_polys > 100_000 || total > 400_000 {
                            continue;
                        }
                        let counts_off = 32;
                        let pts_off = counts_off + n_polys * 4;
                        let mut rings = Vec::with_capacity(n_polys);
                        let mut cursor = 0usize;
                        for i in 0..n_polys {
                            let co = counts_off + i * 4;
                            if co + 4 > rec.len() {
                                break;
                            }
                            let c = u32_at(rec, co) as usize;
                            let ring = if wide {
                                Player::pts32(rec, pts_off + cursor * 8, c)
                            } else {
                                Player::pts16(rec, pts_off + cursor * 4, c)
                            };
                            cursor += c;
                            rings.push(ring);
                        }
                        if kind == 8 {
                            p.fill_and_stroke(&mut canvas, &rings, true);
                        } else if !p.dc.pen.null {
                            let w = p.dc.pen_px();
                            for r in rings {
                                let mapped = p.map(&r);
                                canvas.stroke_polyline(&mapped, w, p.dc.pen.color);
                            }
                        }
                    }
                    _ => {}
                }
            }
            95 if rec.len() >= 48 => {
                let style = u32_at(rec, 28);
                p.store(
                    u32_at(rec, 8),
                    Obj::Pen(Pen {
                        color: colorref(u32_at(rec, 40)),
                        width: u32_at(rec, 32) as f32,
                        null: style & 0xF == 5,
                    }),
                );
            }
            _ => {}
        }
    }

    let (px, w, h) = canvas.downsample(super::SS);
    Some((px, w, h))
}

#[cfg(test)]
mod polypoly_size_tests {
    use super::render;

    /// 88-byte EMR_HEADER (`iType=1`, `nSize=88`, bounds 0..100, `" EMF"`
    /// signature at 40) + raw records + EMR_EOF.
    fn emf_with(records: &[Vec<u8>]) -> Vec<u8> {
        let mut emf = Vec::new();
        emf.extend_from_slice(&1u32.to_le_bytes());
        emf.extend_from_slice(&88u32.to_le_bytes());
        emf.extend_from_slice(&0i32.to_le_bytes()); // rclBounds
        emf.extend_from_slice(&0i32.to_le_bytes());
        emf.extend_from_slice(&100i32.to_le_bytes());
        emf.extend_from_slice(&100i32.to_le_bytes());
        emf.extend_from_slice(&[0u8; 16]); // rclFrame
        emf.extend_from_slice(b" EMF"); // dSignature @40
        emf.extend_from_slice(&[0u8; 44]);
        assert_eq!(emf.len(), 88);
        for rec in records {
            emf.extend_from_slice(rec);
        }
        emf.extend_from_slice(&14u32.to_le_bytes()); // EMR_EOF
        emf.extend_from_slice(&8u32.to_le_bytes());
        emf
    }

    /// Record with `iType`/`nSize` header + body (body length %4).
    fn record(ty: u32, body: &[u8]) -> Vec<u8> {
        let mut rec = Vec::with_capacity(8 + body.len());
        rec.extend_from_slice(&ty.to_le_bytes());
        rec.extend_from_slice(&((8 + body.len()) as u32).to_le_bytes());
        rec.extend_from_slice(body);
        rec
    }

    /// `nSize = 28` (bounds + nPolys only): the arm guard accepts 28 but
    /// `7 | 8` reads `u32_at(rec, 28)` — `range end index 32 out of len 28`.
    #[test]
    fn a_28_byte_polypolyline_record_is_skipped_not_oob() {
        let rec = record(7, &[0u8; 20]);
        assert_eq!(rec.len(), 28);
        assert!(render(&emf_with(&[rec])).is_some());
    }

    /// `iType = 91` maps to kind 8 the same way (`ty - 83`).
    #[test]
    fn a_28_byte_polypolygon_record_is_skipped_not_oob() {
        let rec = record(91, &[0u8; 20]);
        assert_eq!(rec.len(), 28);
        assert!(render(&emf_with(&[rec])).is_some());
    }

    /// A well-formed record (nPolys + total + counts fit in 36 bytes) still
    /// goes through the reads at 24/28 — the fix must not skip it.
    #[test]
    fn a_full_size_polypolyline_record_still_renders() {
        let mut body = vec![0u8; 16]; // bounds
        body.extend_from_slice(&1u32.to_le_bytes()); // nPolys @24
        body.extend_from_slice(&1u32.to_le_bytes()); // total  @28
        body.extend_from_slice(&0u32.to_le_bytes()); // counts[0] @32 (empty ring)
        let rec = record(7, &body);
        assert_eq!(rec.len(), 36);
        assert!(render(&emf_with(&[rec])).is_some());
    }

    /// A per-ring point count is a raw u32 (`counts[0] @32`). `pts32` sized
    /// its vector straight from that field: `u32::MAX` requests a ~34 GB
    /// capacity and the allocator aborts the process. The `total > 400_000`
    /// check never bounds individual ring counts. Capacity must come from
    /// the bytes actually present.
    #[test]
    fn a_hostile_ring_point_count_is_sized_from_the_record_32bit() {
        let mut body = vec![0u8; 16]; // bounds
        body.extend_from_slice(&1u32.to_le_bytes()); // nPolys @24
        body.extend_from_slice(&1u32.to_le_bytes()); // total @28 (claimed, unused)
        body.extend_from_slice(&u32::MAX.to_le_bytes()); // counts[0] @32 ← defect
        body.extend_from_slice(&1i32.to_le_bytes()); // the one real point
        body.extend_from_slice(&2i32.to_le_bytes());
        let rec = record(7, &body);
        let pts = super::Player::pts32(&rec, 36, u32::MAX as usize);
        assert!(
            pts.capacity() <= rec.len() / 8 + 1,
            "pre-allocated {} slots",
            pts.capacity()
        );
        assert_eq!(pts, vec![[1.0, 2.0]]);
    }

    /// Same defect through `pts16` (non-wide records, iType 90 → kind 7).
    #[test]
    fn a_hostile_ring_point_count_is_sized_from_the_record_16bit() {
        let mut body = vec![0u8; 16]; // bounds
        body.extend_from_slice(&1u32.to_le_bytes()); // nPolys @24
        body.extend_from_slice(&1u32.to_le_bytes()); // total @28
        body.extend_from_slice(&u32::MAX.to_le_bytes()); // counts[0] @32 ← defect
        body.extend_from_slice(&3i16.to_le_bytes()); // the one real point
        body.extend_from_slice(&4i16.to_le_bytes());
        let rec = record(90, &body);
        let pts = super::Player::pts16(&rec, 36, u32::MAX as usize);
        assert!(
            pts.capacity() <= rec.len() / 4 + 1,
            "pre-allocated {} slots",
            pts.capacity()
        );
        assert_eq!(pts, vec![[3.0, 4.0]]);
    }

    /// End-to-end: the hostile count reaches `render` via EMR_POLYPOLYLINE
    /// and must still render the points that exist, without dying.
    #[test]
    fn a_hostile_ring_count_still_renders_the_points_that_exist() {
        let mut body = vec![0u8; 16]; // bounds
        body.extend_from_slice(&1u32.to_le_bytes()); // nPolys @24
        body.extend_from_slice(&1u32.to_le_bytes()); // total @28 (≤ cap, claimed)
        body.extend_from_slice(&u32::MAX.to_le_bytes()); // counts[0] @32 ← defect
        body.extend_from_slice(&0i32.to_le_bytes()); // point (0,0)
        body.extend_from_slice(&0i32.to_le_bytes());
        body.extend_from_slice(&50i32.to_le_bytes()); // point (50,0)
        body.extend_from_slice(&0i32.to_le_bytes());
        let rec = record(7, &body);
        assert!(render(&emf_with(&[rec])).is_some());
    }
}
