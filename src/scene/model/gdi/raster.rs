// CPU raster canvas for metafile playback.
//
// An RGBA8 pixel buffer with the primitive set the EMF/WMF players need:
// even-odd polygon fill, polyline stroking (expanded to quads), triangle
// fill (glyph outlines arrive pre-triangulated), axis-aligned rect fill,
// stretched RGBA blits, and a rectangular clip. Rendering happens at a
// supersampled resolution and is box-downsampled once at the end, which
// anti-aliases every primitive uniformly.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub [u8; 4]);

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
    /// Current clip rectangle in canvas pixels (x0, y0, x1, y1), half-open.
    pub clip: (f32, f32, f32, f32),
    /// Canvas pixels per output pixel, across and down.
    pub ss: usize,
}

impl Canvas {
    /// A canvas of `w`×`h` pixels, `ss` of them to an output pixel across.
    pub fn new(w: usize, h: usize, ss: usize) -> Self {
        // White background — OLE presentations are recorded against a white
        // page, and AutoCAD draws OLE frames on white as well.
        Canvas {
            w,
            h,
            px: vec![255u8; w * h * 4],
            clip: (0.0, 0.0, w as f32, h as f32),
            ss,
        }
    }

    #[inline]
    fn put(&mut self, x: usize, y: usize, c: Rgba) {
        let i = (y * self.w + x) * 4;
        self.px[i..i + 4].copy_from_slice(&c.0);
    }

    /// Fill a polygon set with the even-odd rule. `polys` are closed rings in
    /// canvas pixel coordinates; all rings participate in one parity test, so
    /// holes work the way GDI's ALTERNATE fill mode does.
    pub fn fill_polys(&mut self, polys: &[Vec<[f32; 2]>], c: Rgba) {
        let mut ymin = f32::MAX;
        let mut ymax = f32::MIN;
        for p in polys {
            for v in p {
                ymin = ymin.min(v[1]);
                ymax = ymax.max(v[1]);
            }
        }
        if !ymin.is_finite() || !ymax.is_finite() {
            return;
        }
        let y0 = (ymin.max(self.clip.1).floor() as isize).max(0) as usize;
        let y1 = (ymax.min(self.clip.3).ceil() as isize).max(0) as usize;
        let y1 = y1.min(self.h);
        let mut xs: Vec<f32> = Vec::new();
        for y in y0..y1 {
            let yc = y as f32 + 0.5;
            xs.clear();
            for p in polys {
                let n = p.len();
                if n < 3 {
                    continue;
                }
                for i in 0..n {
                    let a = p[i];
                    let b = p[(i + 1) % n];
                    if (a[1] <= yc && b[1] > yc) || (b[1] <= yc && a[1] > yc) {
                        let t = (yc - a[1]) / (b[1] - a[1]);
                        xs.push(a[0] + t * (b[0] - a[0]));
                    }
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.chunks_exact(2) {
                let xa = pair[0].max(self.clip.0).max(0.0).round() as usize;
                let xb = pair[1].min(self.clip.2).min(self.w as f32).round() as usize;
                for x in xa..xb.min(self.w) {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// Fill a flat triangle list (groups of 3 vertices) — used for glyphs.
    pub fn fill_tris(&mut self, tris: &[[f32; 2]], c: Rgba) {
        for t in tris.chunks_exact(3) {
            let (a, b, d) = (t[0], t[1], t[2]);
            let ymin = a[1].min(b[1]).min(d[1]).max(self.clip.1).max(0.0);
            let ymax = a[1].max(b[1]).max(d[1]).min(self.clip.3).min(self.h as f32);
            let det = (b[0] - a[0]) * (d[1] - a[1]) - (d[0] - a[0]) * (b[1] - a[1]);
            if det.abs() < 1e-12 {
                continue;
            }
            let y0 = ymin.floor() as usize;
            let y1 = ymax.ceil() as usize;
            for y in y0..y1.min(self.h) {
                let yc = y as f32 + 0.5;
                // Row span via edge intersections — cheaper than barycentric
                // over the bbox for the thin triangles glyphs produce.
                let mut xmin = f32::MAX;
                let mut xmax = f32::MIN;
                let edges = [(a, b), (b, d), (d, a)];
                for (p, q) in edges {
                    if (p[1] <= yc && q[1] > yc) || (q[1] <= yc && p[1] > yc) {
                        let t = (yc - p[1]) / (q[1] - p[1]);
                        let x = p[0] + t * (q[0] - p[0]);
                        xmin = xmin.min(x);
                        xmax = xmax.max(x);
                    }
                }
                if xmin > xmax {
                    continue;
                }
                let xa = xmin.max(self.clip.0).max(0.0).round() as usize;
                let xb = xmax.min(self.clip.2).min(self.w as f32).round() as usize;
                for x in xa..xb {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// Stroke an open polyline with the given width by expanding each segment
    /// into a quad (plus square joints at interior vertices).
    ///
    /// A horizontal or vertical stretch of a pen one output pixel wide or
    /// thinner lights a whole row or column of output pixels, as the system
    /// plays a picture at the size it is shown: it moves across to the
    /// centres of the output pixels it falls in. Centred on a pixel edge,
    /// such a line would cover two half pixels and show at half its colour.
    /// Slanted and curved stretches keep their place, smoothed.
    pub fn stroke_polyline(&mut self, pts: &[[f32; 2]], width: f32, c: Rgba) {
        let ss = self.ss as f32;
        let hairline = width <= ss;
        let centre = |v: f32| ((v / ss).floor() + 0.5) * ss;
        let w = width.max(1.0) * 0.5;
        for seg in pts.windows(2) {
            let (mut a, mut b) = (seg[0], seg[1]);
            if hairline && (a[1] - b[1]).abs() < 1e-3 {
                let y = centre(a[1]);
                (a[1], b[1]) = (y, y);
            } else if hairline && (a[0] - b[0]).abs() < 1e-3 {
                let x = centre(a[0]);
                (a[0], b[0]) = (x, x);
            }
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 {
                continue;
            }
            let nx = -dy / len * w;
            let ny = dx / len * w;
            // extend caps by half-width so joints don't leave pinholes
            let ex = dx / len * w;
            let ey = dy / len * w;
            let quad = vec![
                [a[0] + nx - ex, a[1] + ny - ey],
                [b[0] + nx + ex, b[1] + ny + ey],
                [b[0] - nx + ex, b[1] - ny + ey],
                [a[0] - nx - ex, a[1] - ny - ey],
            ];
            self.fill_polys(std::slice::from_ref(&quad), c);
        }
    }

    pub fn fill_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgba) {
        let r = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        self.fill_polys(std::slice::from_ref(&r), c);
    }

    /// Blit `src` (RGBA, sw×sh) stretched into the destination rectangle,
    /// nearest-neighbour sampled from the `sx..sx+scx` × `sy..sy+scy` source
    /// window. Negative destination extents flip the image. Source alpha
    /// composites over the canvas.
    #[allow(clippy::too_many_arguments)]
    pub fn blit(
        &mut self,
        src: &[u8],
        sw: usize,
        sh: usize,
        sx: f32,
        sy: f32,
        scx: f32,
        scy: f32,
        dx0: f32,
        dy0: f32,
        dx1: f32,
        dy1: f32,
    ) {
        if sw == 0 || sh == 0 || (dx1 - dx0).abs() < 0.5 || (dy1 - dy0).abs() < 0.5 {
            return;
        }
        let (xa, xb) = (dx0.min(dx1), dx0.max(dx1));
        let (ya, yb) = (dy0.min(dy1), dy0.max(dy1));
        let x0 = xa.max(self.clip.0).max(0.0).floor() as usize;
        let x1 = (xb.min(self.clip.2).min(self.w as f32).ceil() as usize).min(self.w);
        let y0 = ya.max(self.clip.1).max(0.0).floor() as usize;
        let y1 = (yb.min(self.clip.3).min(self.h as f32).ceil() as usize).min(self.h);
        for y in y0..y1 {
            let fy = ((y as f32 + 0.5) - dy0) / (dy1 - dy0);
            let syf = sy + fy * scy;
            let syi = syf.floor() as isize;
            if syi < 0 || syi >= sh as isize {
                continue;
            }
            for x in x0..x1 {
                let fx = ((x as f32 + 0.5) - dx0) / (dx1 - dx0);
                let sxf = sx + fx * scx;
                let sxi = sxf.floor() as isize;
                if sxi < 0 || sxi >= sw as isize {
                    continue;
                }
                let si = (syi as usize * sw + sxi as usize) * 4;
                let sa = src[si + 3] as u32;
                if sa == 0 {
                    continue;
                }
                let di = (y * self.w + x) * 4;
                if sa == 255 {
                    self.px[di..di + 4].copy_from_slice(&src[si..si + 4]);
                } else {
                    for k in 0..3 {
                        let d = self.px[di + k] as u32;
                        let s = src[si + k] as u32;
                        self.px[di + k] = ((s * sa + d * (255 - sa)) / 255) as u8;
                    }
                    self.px[di + 3] = 255;
                }
            }
        }
    }

    /// Box-downsample by an integer factor into a fresh RGBA buffer.
    pub fn downsample(&self, factor: usize) -> (Vec<u8>, u32, u32) {
        if factor <= 1 {
            return (self.px.clone(), self.w as u32, self.h as u32);
        }
        let ow = (self.w / factor).max(1);
        let oh = (self.h / factor).max(1);
        let mut out = vec![0u8; ow * oh * 4];
        let n = (factor * factor) as u32;
        for oy in 0..oh {
            for ox in 0..ow {
                let mut acc = [0u32; 4];
                for fy in 0..factor {
                    for fx in 0..factor {
                        let i = ((oy * factor + fy) * self.w + ox * factor + fx) * 4;
                        for k in 0..4 {
                            acc[k] += self.px[i + k] as u32;
                        }
                    }
                }
                let o = (oy * ow + ox) * 4;
                for k in 0..4 {
                    out[o + k] = (acc[k] / n) as u8;
                }
            }
        }
        (out, ow as u32, oh as u32)
    }
}
