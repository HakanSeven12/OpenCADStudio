// Minimal GDI metafile playback: rasterize the EMF / WMF presentation
// pictures embedded in OLE2FRAME entities into an RGBA image.
//
// This is not a general GDI implementation — it covers the record set that
// Office-generated OLE previews actually use: window/viewport + world
// transforms, pens/brushes/fonts, polygon & polyline drawing, rectangle and
// ellipse primitives, paths, DIB blits (including brush-only PATCOPY fills),
// and text (EXTTEXTOUTW / TEXTOUT with per-char advances). Text glyphs come
// from the system-font outline cache the CAD text renderer already maintains,
// so scripts and fallback behave exactly like drawing text in the editor.

pub mod dib;
mod emf;
mod raster;
mod text;
mod wmf;

pub use raster::{Canvas, Rgba};

/// Output long-side target in pixels for a rasterized metafile.
const TARGET_DIM: usize = 1600;
/// Supersampling factor; the canvas renders at TARGET_DIM×SS and is
/// box-downsampled once at the end for uniform anti-aliasing.
const SS: usize = 2;

/// 2-D affine transform (GDI XFORM layout).
#[derive(Clone, Copy, Debug)]
pub struct Xform {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub dx: f32,
    pub dy: f32,
}

impl Xform {
    pub const IDENTITY: Xform = Xform {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        dx: 0.0,
        dy: 0.0,
    };

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.m11 * x + self.m21 * y + self.dx,
            self.m12 * x + self.m22 * y + self.dy,
        )
    }

    /// self ∘ other (apply `other` first, then `self`).
    pub fn mul(&self, o: &Xform) -> Xform {
        Xform {
            m11: o.m11 * self.m11 + o.m12 * self.m21,
            m12: o.m11 * self.m12 + o.m12 * self.m22,
            m21: o.m21 * self.m11 + o.m22 * self.m21,
            m22: o.m21 * self.m12 + o.m22 * self.m22,
            dx: o.dx * self.m11 + o.dy * self.m21 + self.dx,
            dy: o.dx * self.m12 + o.dy * self.m22 + self.dy,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Pen {
    pub color: Rgba,
    /// Logical-unit width; 0 = cosmetic (1 device pixel).
    pub width: f32,
    /// PS_NULL — draws nothing.
    pub null: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Brush {
    pub color: Rgba,
    /// BS_NULL / HOLLOW — fills nothing.
    pub null: bool,
}

#[derive(Clone, Debug)]
pub struct Font {
    /// Logical-unit height; negative = em height, positive = cell height.
    pub height: f32,
    /// Escapement angle in 0.1°, counter-clockwise.
    pub escapement: f32,
    pub facename: String,
    /// LOGFONT charset (WMF ANSI strings decode through it).
    pub charset: u8,
}

impl Default for Font {
    fn default() -> Self {
        Font {
            height: -12.0,
            escapement: 0.0,
            facename: String::new(),
            charset: 0,
        }
    }
}

/// One GDI object slot (metafiles address objects by table index).
#[derive(Clone, Debug)]
pub enum Obj {
    Pen(Pen),
    Brush(Brush),
    Font(Font),
    /// Created-but-unsupported object kind, occupies its slot.
    Other,
}

/// Playback device context.
pub struct Dc {
    pub world: Xform,
    pub win_org: (f32, f32),
    pub win_ext: (f32, f32),
    pub vp_org: (f32, f32),
    pub vp_ext: (f32, f32),
    /// device → canvas mapping.
    pub dev_org: (f32, f32),
    pub dev_scale: (f32, f32),
    pub pen: Pen,
    pub brush: Brush,
    pub font: Font,
    pub text_color: Rgba,
    pub bk_color: Rgba,
    /// 1 = TRANSPARENT, 2 = OPAQUE.
    pub bk_mode: u32,
    pub text_align: u32,
    /// Current position (logical), for MOVETO/LINETO chains.
    pub pos: (f32, f32),
    saved: Vec<DcSnapshot>,
}

struct DcSnapshot {
    world: Xform,
    win_org: (f32, f32),
    win_ext: (f32, f32),
    vp_org: (f32, f32),
    vp_ext: (f32, f32),
    pen: Pen,
    brush: Brush,
    font: Font,
    text_color: Rgba,
    bk_color: Rgba,
    bk_mode: u32,
    text_align: u32,
    pos: (f32, f32),
    clip: (f32, f32, f32, f32),
}

impl Dc {
    pub fn new() -> Self {
        Dc {
            world: Xform::IDENTITY,
            win_org: (0.0, 0.0),
            win_ext: (1.0, 1.0),
            vp_org: (0.0, 0.0),
            vp_ext: (1.0, 1.0),
            dev_org: (0.0, 0.0),
            dev_scale: (1.0, 1.0),
            pen: Pen {
                color: Rgba([0, 0, 0, 255]),
                width: 0.0,
                null: false,
            },
            brush: Brush {
                color: Rgba([255, 255, 255, 255]),
                null: false,
            },
            font: Font::default(),
            text_color: Rgba([0, 0, 0, 255]),
            bk_color: Rgba([255, 255, 255, 255]),
            bk_mode: 2,
            text_align: 0,
            pos: (0.0, 0.0),
            saved: Vec::new(),
        }
    }

    /// Logical point → canvas pixel.
    pub fn to_canvas(&self, x: f32, y: f32) -> [f32; 2] {
        let (px, py) = self.world.apply(x, y);
        let dx = (px - self.win_org.0) * (self.vp_ext.0 / self.win_ext.0) + self.vp_org.0;
        let dy = (py - self.win_org.1) * (self.vp_ext.1 / self.win_ext.1) + self.vp_org.1;
        [
            (dx - self.dev_org.0) * self.dev_scale.0,
            (dy - self.dev_org.1) * self.dev_scale.1,
        ]
    }

    /// Average |logical → canvas| scale factor (pen widths, font sizes).
    pub fn scale_avg(&self) -> f32 {
        let wsx = (self.world.m11 * self.world.m11 + self.world.m12 * self.world.m12).sqrt();
        let wsy = (self.world.m21 * self.world.m21 + self.world.m22 * self.world.m22).sqrt();
        let sx = (self.vp_ext.0 / self.win_ext.0 * self.dev_scale.0).abs() * wsx.max(1e-12);
        let sy = (self.vp_ext.1 / self.win_ext.1 * self.dev_scale.1).abs() * wsy.max(1e-12);
        ((sx + sy) * 0.5).max(1e-12)
    }

    /// Vertical |logical → canvas| scale (font heights).
    pub fn scale_y(&self) -> f32 {
        let wsy = (self.world.m21 * self.world.m21 + self.world.m22 * self.world.m22)
            .sqrt()
            .max(1e-12);
        (self.vp_ext.1 / self.win_ext.1 * self.dev_scale.1).abs() * wsy
    }

    pub fn pen_px(&self) -> f32 {
        if self.pen.width <= 0.0 {
            // Cosmetic pen: one device pixel regardless of transforms — at the
            // supersampled canvas resolution that's SS pixels.
            SS as f32
        } else {
            (self.pen.width * self.scale_avg()).max(SS as f32)
        }
    }

    pub fn save(&mut self, canvas: &Canvas) {
        self.saved.push(DcSnapshot {
            world: self.world,
            win_org: self.win_org,
            win_ext: self.win_ext,
            vp_org: self.vp_org,
            vp_ext: self.vp_ext,
            pen: self.pen,
            brush: self.brush,
            font: self.font.clone(),
            text_color: self.text_color,
            bk_color: self.bk_color,
            bk_mode: self.bk_mode,
            text_align: self.text_align,
            pos: self.pos,
            clip: canvas.clip,
        });
    }

    pub fn restore(&mut self, canvas: &mut Canvas) {
        if let Some(s) = self.saved.pop() {
            self.world = s.world;
            self.win_org = s.win_org;
            self.win_ext = s.win_ext;
            self.vp_org = s.vp_org;
            self.vp_ext = s.vp_ext;
            self.pen = s.pen;
            self.brush = s.brush;
            self.font = s.font;
            self.text_color = s.text_color;
            self.bk_color = s.bk_color;
            self.bk_mode = s.bk_mode;
            self.text_align = s.text_align;
            self.pos = s.pos;
            canvas.clip = s.clip;
        }
    }
}

/// COLORREF (0x00BBGGRR) → RGBA.
pub fn colorref(v: u32) -> Rgba {
    Rgba([
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
        255,
    ])
}

/// Stock object index (SELECTOBJECT with the 0x8000_0000 flag) → object.
pub fn stock_obj(idx: u32) -> Option<Obj> {
    let white = Rgba([255, 255, 255, 255]);
    let black = Rgba([0, 0, 0, 255]);
    let grey = |v: u8| Rgba([v, v, v, 255]);
    Some(match idx {
        0 => Obj::Brush(Brush {
            color: white,
            null: false,
        }), // WHITE_BRUSH
        1 => Obj::Brush(Brush {
            color: grey(0xC0),
            null: false,
        }), // LTGRAY
        2 => Obj::Brush(Brush {
            color: grey(0x80),
            null: false,
        }), // GRAY
        3 => Obj::Brush(Brush {
            color: grey(0x40),
            null: false,
        }), // DKGRAY
        4 => Obj::Brush(Brush {
            color: black,
            null: false,
        }), // BLACK_BRUSH
        5 => Obj::Brush(Brush {
            color: white,
            null: true,
        }), // NULL_BRUSH
        6 => Obj::Pen(Pen {
            color: white,
            width: 0.0,
            null: false,
        }), // WHITE_PEN
        7 => Obj::Pen(Pen {
            color: black,
            width: 0.0,
            null: false,
        }), // BLACK_PEN
        8 => Obj::Pen(Pen {
            color: black,
            width: 0.0,
            null: true,
        }), // NULL_PEN
        10..=17 => Obj::Font(Font::default()),
        _ => return None,
    })
}

/// Size the supersampled canvas for a device-space content box, and yield the
/// device→canvas mapping. Returns `(canvas, dev_org, dev_scale)`.
fn canvas_for_box(w: f32, h: f32) -> Option<(Canvas, (f32, f32))> {
    if !(w.is_finite() && h.is_finite()) || w.abs() < 1e-9 || h.abs() < 1e-9 {
        return None;
    }
    let long = w.abs().max(h.abs());
    let k = (TARGET_DIM * SS) as f32 / long;
    let cw = ((w.abs() * k).round() as usize).clamp(SS, TARGET_DIM * SS);
    let ch = ((h.abs() * k).round() as usize).clamp(SS, TARGET_DIM * SS);
    Some((Canvas::new(cw, ch), (k * w.signum(), k * h.signum())))
}

/// Rasterize an EMF picture. Returns RGBA pixels + dimensions.
pub fn render_emf(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    emf::render(data)
}

/// Rasterize a WMF picture, placeable or standard. WMFC-wrapped enhanced
/// metafiles never reach here — opencadcodec's presentation extraction already
/// reassembles those to EMF.
pub fn render_wmf(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    wmf::render(data)
}
