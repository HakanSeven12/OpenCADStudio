// WMF (16-bit Windows metafile) record player, plus the WMFC → EMF
// re-assembly used when a memory WMF is only a compatibility wrapper around
// an embedded enhanced metafile (the form Office writes into OlePres000).

use super::raster::{Canvas, Rgba};
use super::{canvas_for_box, colorref, dib, text, Brush, Dc, Font, Obj, Pen};

#[inline]
fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(d[o..o + 2].try_into().unwrap())
}
#[inline]
fn i16_at(d: &[u8], o: usize) -> i16 {
    i16::from_le_bytes(d[o..o + 2].try_into().unwrap())
}
#[inline]
fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

const PLACEABLE_KEY: u32 = 0x9AC6_CDD7;

/// Strip a placeable header if present. Returns `(records_region, bbox)` where
/// the region starts at the standard WMF header and bbox is the placeable
/// bounds (logical units), if any.
fn strip_placeable(data: &[u8]) -> (&[u8], Option<(f32, f32, f32, f32)>) {
    if data.len() >= 22 && u32_at(data, 0) == PLACEABLE_KEY {
        let l = i16_at(data, 6) as f32;
        let t = i16_at(data, 8) as f32;
        let r = i16_at(data, 10) as f32;
        let b = i16_at(data, 12) as f32;
        (&data[22..], Some((l, t, r, b)))
    } else {
        (data, None)
    }
}

/// Validate the standard WMF header, returning the offset of the first record.
fn header_ok(d: &[u8]) -> Option<usize> {
    if d.len() < 18 {
        return None;
    }
    let ty = u16_at(d, 0);
    let hs = u16_at(d, 2) as usize;
    if (ty != 1 && ty != 2) || hs != 9 {
        return None;
    }
    Some(hs * 2)
}

/// Iterate records as `(function, params)` slices.
struct Records<'a> {
    d: &'a [u8],
    off: usize,
}

impl<'a> Iterator for Records<'a> {
    type Item = (u16, &'a [u8]);
    fn next(&mut self) -> Option<(u16, &'a [u8])> {
        if self.off + 6 > self.d.len() {
            return None;
        }
        let size_w = u32_at(self.d, self.off) as usize;
        let func = u16_at(self.d, self.off + 4);
        let size_b = size_w.checked_mul(2)?;
        if size_b < 6 || self.off + size_b > self.d.len() {
            return None;
        }
        let params = &self.d[self.off + 6..self.off + size_b];
        self.off += size_b;
        if func == 0 {
            return None; // META_EOF
        }
        Some((func, params))
    }
}

struct Player {
    dc: Dc,
    objects: Vec<Option<Obj>>,
}

impl Player {
    /// WMF object slots are assigned to the lowest free index.
    fn create(&mut self, obj: Obj) {
        if let Some(slot) = self.objects.iter_mut().find(|s| s.is_none()) {
            *slot = Some(obj);
        } else {
            self.objects.push(Some(obj));
        }
    }

    fn select(&mut self, idx: u16) {
        match self.objects.get(idx as usize).and_then(|o| o.clone()) {
            Some(Obj::Pen(p)) => self.dc.pen = p,
            Some(Obj::Brush(b)) => self.dc.brush = b,
            Some(Obj::Font(f)) => self.dc.font = f,
            _ => {}
        }
    }

    fn map(&self, pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
        pts.iter().map(|p| self.dc.to_canvas(p[0], p[1])).collect()
    }

    fn fill_and_stroke(&mut self, canvas: &mut Canvas, rings: &[Vec<[f32; 2]>]) {
        let mapped: Vec<Vec<[f32; 2]>> = rings.iter().map(|r| self.map(r)).collect();
        if !self.dc.brush.null {
            canvas.fill_polys(&mapped, self.dc.brush.color);
        }
        if !self.dc.pen.null {
            let w = self.dc.pen_px();
            for r in &mapped {
                if r.len() >= 2 {
                    let mut closed = r.clone();
                    closed.push(r[0]);
                    canvas.stroke_polyline(&closed, w, self.dc.pen.color);
                }
            }
        }
    }
}

/// Blit a packed DIB from a blit record. `dest`/`src` rectangles are in
/// logical / source-pixel units respectively.
#[allow(clippy::too_many_arguments)]
fn blit_dib(
    canvas: &mut Canvas,
    dc: &Dc,
    dibdata: &[u8],
    xd: f32,
    yd: f32,
    wd: f32,
    hd: f32,
    sx: f32,
    sy: f32,
    sw: f32,
    sh: f32,
) {
    let Some((px, w, h)) = dib::decode_packed(dibdata) else {
        return;
    };
    let a = dc.to_canvas(xd, yd);
    let b = dc.to_canvas(xd + wd, yd + hd);
    let (sw, sh) = if sw <= 0.0 || sh <= 0.0 {
        (w as f32, h as f32)
    } else {
        (sw, sh)
    };
    canvas.blit(
        &px, w as usize, h as usize, sx, sy, sw, sh, a[0], a[1], b[0], b[1],
    );
}

pub fn render(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let (data, placeable_box) = strip_placeable(data);
    let first = header_ok(data)?;

    // Establish the logical picture box: the first window org/ext pair wins,
    // falling back to the placeable bounds.
    let mut org: Option<(f32, f32)> = None;
    let mut ext: Option<(f32, f32)> = None;
    for (func, p) in (Records {
        d: data,
        off: first,
    }) {
        match func {
            0x020B if p.len() >= 4 && org.is_none() => {
                org = Some((i16_at(p, 2) as f32, i16_at(p, 0) as f32));
            }
            0x020C if p.len() >= 4 && ext.is_none() => {
                ext = Some((i16_at(p, 2) as f32, i16_at(p, 0) as f32));
            }
            _ => {}
        }
        if org.is_some() && ext.is_some() {
            break;
        }
    }
    let (wx, wy, we, he) = match (org, ext, placeable_box) {
        (Some((x, y)), Some((w, h)), _) => (x, y, w, h),
        (_, _, Some((l, t, r, b))) => (l, t, r - l, b - t),
        _ => return None,
    };
    if we == 0.0 || he == 0.0 {
        return None;
    }

    let (mut canvas, _) = canvas_for_box(we, he)?;
    let mut p = Player {
        dc: Dc::new(),
        objects: Vec::new(),
    };
    // Window → viewport maps straight onto the canvas; the device→canvas
    // stage stays identity. The window's origin goes to the canvas's top
    // left whatever the signs of its extent, as the system plays it: a
    // negative height is a picture drawn with y up, and it shows upright
    // (#2625).
    p.dc.win_org = (wx, wy);
    p.dc.win_ext = (we, he);
    p.dc.vp_org = (0.0, 0.0);
    p.dc.vp_ext = (canvas.w as f32, canvas.h as f32);

    for (func, prm) in (Records {
        d: data,
        off: first,
    }) {
        match func {
            0x020B if prm.len() >= 4 => {
                p.dc.win_org = (i16_at(prm, 2) as f32, i16_at(prm, 0) as f32)
            }
            0x020C if prm.len() >= 4 => {
                let (cx, cy) = (i16_at(prm, 2) as f32, i16_at(prm, 0) as f32);
                if cx != 0.0 && cy != 0.0 {
                    p.dc.win_ext = (cx, cy);
                }
            }
            0x0201 if prm.len() >= 4 => p.dc.bk_color = colorref(u32_at(prm, 0)),
            0x0102 if prm.len() >= 2 => p.dc.bk_mode = u16_at(prm, 0) as u32,
            0x0209 if prm.len() >= 4 => p.dc.text_color = colorref(u32_at(prm, 0)),
            0x012E if prm.len() >= 2 => p.dc.text_align = u16_at(prm, 0) as u32,
            0x001E => p.dc.save(&canvas),
            0x0127 => p.dc.restore(&mut canvas),
            0x0416 if prm.len() >= 8 => {
                let a = p.dc.to_canvas(i16_at(prm, 6) as f32, i16_at(prm, 4) as f32);
                let b = p.dc.to_canvas(i16_at(prm, 2) as f32, i16_at(prm, 0) as f32);
                let c = &mut canvas.clip;
                c.0 = c.0.max(a[0].min(b[0]));
                c.1 = c.1.max(a[1].min(b[1]));
                c.2 = c.2.min(a[0].max(b[0]));
                c.3 = c.3.min(a[1].max(b[1]));
            }
            0x0214 if prm.len() >= 4 => p.dc.pos = (i16_at(prm, 2) as f32, i16_at(prm, 0) as f32),
            0x0213 if prm.len() >= 4 => {
                let to = (i16_at(prm, 2) as f32, i16_at(prm, 0) as f32);
                if !p.dc.pen.null {
                    let seg = p.map(&[[p.dc.pos.0, p.dc.pos.1], [to.0, to.1]]);
                    let w = p.dc.pen_px();
                    canvas.stroke_polyline(&seg, w, p.dc.pen.color);
                }
                p.dc.pos = to;
            }
            0x0324 | 0x0325 if prm.len() >= 2 => {
                let n = u16_at(prm, 0) as usize;
                if n == 0 || 2 + n * 4 > prm.len() {
                    continue;
                }
                let pts: Vec<[f32; 2]> = (0..n)
                    .map(|i| [i16_at(prm, 2 + i * 4) as f32, i16_at(prm, 4 + i * 4) as f32])
                    .collect();
                if func == 0x0324 {
                    p.fill_and_stroke(&mut canvas, std::slice::from_ref(&pts));
                } else if !p.dc.pen.null {
                    let mapped = p.map(&pts);
                    let w = p.dc.pen_px();
                    canvas.stroke_polyline(&mapped, w, p.dc.pen.color);
                }
            }
            0x0538 if prm.len() >= 2 => {
                let n_polys = u16_at(prm, 0) as usize;
                if n_polys == 0 || 2 + n_polys * 2 > prm.len() {
                    continue;
                }
                let mut rings = Vec::with_capacity(n_polys);
                let mut pt_off = 2 + n_polys * 2;
                for i in 0..n_polys {
                    let c = u16_at(prm, 2 + i * 2) as usize;
                    if pt_off + c * 4 > prm.len() {
                        break;
                    }
                    rings.push(
                        (0..c)
                            .map(|j| {
                                [
                                    i16_at(prm, pt_off + j * 4) as f32,
                                    i16_at(prm, pt_off + 2 + j * 4) as f32,
                                ]
                            })
                            .collect::<Vec<_>>(),
                    );
                    pt_off += c * 4;
                }
                p.fill_and_stroke(&mut canvas, &rings);
            }
            0x041B | 0x0418 if prm.len() >= 8 => {
                let (b, r, t, l) = (
                    i16_at(prm, 0) as f32,
                    i16_at(prm, 2) as f32,
                    i16_at(prm, 4) as f32,
                    i16_at(prm, 6) as f32,
                );
                let ring: Vec<[f32; 2]> = if func == 0x041B {
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
                p.fill_and_stroke(&mut canvas, std::slice::from_ref(&ring));
            }
            0x061D if prm.len() >= 12 => {
                // META_PATBLT: brush fill of a rectangle.
                let rop = u32_at(prm, 0);
                let (h, w, y, x) = (
                    i16_at(prm, 4) as f32,
                    i16_at(prm, 6) as f32,
                    i16_at(prm, 8) as f32,
                    i16_at(prm, 10) as f32,
                );
                let color = match rop {
                    0x00F0_0021 | 0x005A_0049 => Some(p.dc.brush.color),
                    0x0000_0042 => Some(Rgba([0, 0, 0, 255])),
                    0x00FF_0062 => Some(Rgba([255, 255, 255, 255])),
                    _ => None,
                };
                if let Some(c) = color {
                    let a = p.dc.to_canvas(x, y);
                    let b = p.dc.to_canvas(x + w, y + h);
                    canvas.fill_rect(
                        a[0].min(b[0]),
                        a[1].min(b[1]),
                        a[0].max(b[0]),
                        a[1].max(b[1]),
                        c,
                    );
                }
            }
            0x02FA if prm.len() >= 10 => {
                let style = u16_at(prm, 0);
                p.create(Obj::Pen(Pen {
                    color: colorref(u32_at(prm, 6)),
                    width: i16_at(prm, 2) as f32,
                    null: style & 0xF == 5,
                }));
            }
            0x02FC if prm.len() >= 6 => {
                let style = u16_at(prm, 0);
                p.create(Obj::Brush(Brush {
                    color: colorref(u32_at(prm, 2)),
                    null: style == 1,
                }));
            }
            0x02FB if prm.len() >= 18 => {
                let mut name = String::new();
                for &b in prm[18..].iter().take(32) {
                    if b == 0 {
                        break;
                    }
                    name.push(b as char);
                }
                p.create(Obj::Font(Font {
                    height: i16_at(prm, 0) as f32,
                    escapement: i16_at(prm, 4) as f32,
                    facename: name,
                    charset: prm[13],
                }));
            }
            // Other object-creating records occupy a slot even when unused.
            0x00F7 | 0x01F9 | 0x0142 | 0x06FF => p.create(Obj::Other),
            0x012D if prm.len() >= 2 => p.select(u16_at(prm, 0)),
            0x01F0 if prm.len() >= 2 => {
                let i = u16_at(prm, 0) as usize;
                if i < p.objects.len() {
                    p.objects[i] = None;
                }
            }
            0x0521 if prm.len() >= 2 => {
                let n = u16_at(prm, 0) as usize;
                let str_end = 2 + n;
                let padded = str_end + (str_end & 1);
                if n == 0 || padded + 4 > prm.len() {
                    continue;
                }
                let s = text::decode_ansi(&prm[2..str_end], &p.dc.font);
                let y = i16_at(prm, padded) as f32;
                let x = i16_at(prm, padded + 2) as f32;
                text::draw_text(&mut canvas, &mut p.dc, x, y, &s, None, None);
            }
            0x0A32 if prm.len() >= 8 => {
                let y = i16_at(prm, 0) as f32;
                let x = i16_at(prm, 2) as f32;
                let n = u16_at(prm, 4) as usize;
                let opts = u16_at(prm, 6);
                let mut off = 8;
                let mut opaque = None;
                if opts & 0x0006 != 0 && prm.len() >= off + 8 {
                    let rect = [
                        i16_at(prm, off) as f32,
                        i16_at(prm, off + 2) as f32,
                        i16_at(prm, off + 4) as f32,
                        i16_at(prm, off + 6) as f32,
                    ];
                    if opts & 0x0002 != 0 {
                        opaque = Some(rect);
                    }
                    off += 8;
                }
                if n == 0 || off + n > prm.len() {
                    continue;
                }
                let s = text::decode_ansi(&prm[off..off + n], &p.dc.font);
                let str_pad = n + (n & 1);
                let dx_off = off + str_pad;
                let dx: Option<Vec<f32>> = if dx_off + n * 2 <= prm.len() {
                    Some((0..n).map(|i| i16_at(prm, dx_off + i * 2) as f32).collect())
                } else {
                    None
                };
                text::draw_text(&mut canvas, &mut p.dc, x, y, &s, dx.as_deref(), opaque);
            }
            0x0F43 if prm.len() >= 22 => {
                let (sh, sw, sy, sx) = (
                    i16_at(prm, 6) as f32,
                    i16_at(prm, 8) as f32,
                    i16_at(prm, 10) as f32,
                    i16_at(prm, 12) as f32,
                );
                let (dh, dw, dy, dx) = (
                    i16_at(prm, 14) as f32,
                    i16_at(prm, 16) as f32,
                    i16_at(prm, 18) as f32,
                    i16_at(prm, 20) as f32,
                );
                blit_dib(
                    &mut canvas,
                    &p.dc,
                    &prm[22..],
                    dx,
                    dy,
                    dw,
                    dh,
                    sx,
                    sy,
                    sw,
                    sh,
                );
            }
            0x0B41 if prm.len() >= 20 => {
                let (sh, sw, sy, sx) = (
                    i16_at(prm, 4) as f32,
                    i16_at(prm, 6) as f32,
                    i16_at(prm, 8) as f32,
                    i16_at(prm, 10) as f32,
                );
                let (dh, dw, dy, dx) = (
                    i16_at(prm, 12) as f32,
                    i16_at(prm, 14) as f32,
                    i16_at(prm, 16) as f32,
                    i16_at(prm, 18) as f32,
                );
                blit_dib(
                    &mut canvas,
                    &p.dc,
                    &prm[20..],
                    dx,
                    dy,
                    dw,
                    dh,
                    sx,
                    sy,
                    sw,
                    sh,
                );
            }
            0x0940 if prm.len() >= 16 => {
                let (sy, sx) = (i16_at(prm, 4) as f32, i16_at(prm, 6) as f32);
                let (dh, dw, dy, dx) = (
                    i16_at(prm, 8) as f32,
                    i16_at(prm, 10) as f32,
                    i16_at(prm, 12) as f32,
                    i16_at(prm, 14) as f32,
                );
                blit_dib(
                    &mut canvas,
                    &p.dc,
                    &prm[16..],
                    dx,
                    dy,
                    dw,
                    dh,
                    sx,
                    sy,
                    dw,
                    dh,
                );
            }
            _ => {}
        }
    }

    let (px, w, h) = canvas.downsample(super::SS);
    Some((px, w, h))
}

#[cfg(test)]
mod tests {
    /// A standard WMF holding `records` (function, 16-bit params).
    fn wmf(records: &[(u16, Vec<i16>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (func, params) in records {
            body.extend_from_slice(&(3 + params.len() as u32).to_le_bytes());
            body.extend_from_slice(&func.to_le_bytes());
            for p in params {
                body.extend_from_slice(&p.to_le_bytes());
            }
        }
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes());
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&9u16.to_le_bytes());
        out.extend_from_slice(&0x0300u16.to_le_bytes());
        out.extend_from_slice(&((9 + body.len() / 2) as u32).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// #2625: a metafile drawn with y up (a negative window height) shows
    /// upright, as the system draws it: what is high in its coordinates is
    /// at the top of the picture.
    #[test]
    fn a_y_up_metafile_draws_upright() {
        let data = wmf(&[
            (0x020B, vec![100, 0]),  // SetWindowOrg(0, 100)
            (0x020C, vec![-100, 100]), // SetWindowExt(100, -100)
            // CreateBrushIndirect: solid red; SelectObject(0).
            (0x02FC, vec![0, 0x00FF, 0x0000, 0]),
            (0x012D, vec![0]),
            // Rectangle(left 0, top 100, right 100, bottom 60): the upper
            // part in y-up coordinates.
            (0x041B, vec![60, 100, 100, 0]),
        ]);
        let (px, w, h) = super::render(&data).expect("renders");
        let red = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            px[i] > 200 && px[i + 1] < 60 && px[i + 2] < 60
        };
        assert!(red(w / 2, h / 10), "the top of the picture is red");
        assert!(!red(w / 2, h * 9 / 10), "the bottom is not");
    }
}
