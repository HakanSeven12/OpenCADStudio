// LibreCAD LFF stroke-font engine.
//
// Parses LibreCAD Font Format (`.lff`) files and tessellates text into
// world-space 2-D polyline strokes. Replaces the former QCAD CXF engine.
//
// LFF layout:
//   - `# Key: value` headers (Name / LetterSpacing / WordSpacing /
//     LineSpacingFactor).
//   - Glyph blocks `[<hex>] <char>` followed by stroke lines. Each stroke
//     line is a `;`-separated polyline of `x,y` vertices; a vertex written
//     `x,y,A<bulge>` makes the segment to the NEXT vertex a bulge arc
//     (DXF bulge = tan(included_angle / 4)).
//   - A line `C<hex>` inside a glyph includes another glyph's strokes
//     (used to compose accented characters).
//
// Cap height is 9 glyph units, matching the `height / 9.0` text scale used
// throughout the renderer.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

// ── Embedded fonts ─────────────────────────────────────────────────────────

/// Every LibreCAD LFF font, keyed by its file stem (lower-case). Registered
/// under the upper-cased stem and the font's `# Name:` header.
const FONTS_SRC: &[(&str, &str)] = &[
    ("amiri-regular", include_str!("../../assets/fonts/amiri-regular.lff")),
    ("azomix_i", include_str!("../../assets/fonts/azomix_i.lff")),
    ("azomix", include_str!("../../assets/fonts/azomix.lff")),
    ("cursive", include_str!("../../assets/fonts/cursive.lff")),
    ("cyrillic_ii", include_str!("../../assets/fonts/cyrillic_ii.lff")),
    ("gothgbt", include_str!("../../assets/fonts/gothgbt.lff")),
    ("gothgrt", include_str!("../../assets/fonts/gothgrt.lff")),
    ("gothitt", include_str!("../../assets/fonts/gothitt.lff")),
    ("greekc", include_str!("../../assets/fonts/greekc.lff")),
    ("greekcs", include_str!("../../assets/fonts/greekcs.lff")),
    ("greek_ol", include_str!("../../assets/fonts/greek_ol.lff")),
    ("greekp", include_str!("../../assets/fonts/greekp.lff")),
    ("greeks", include_str!("../../assets/fonts/greeks.lff")),
    ("iso3098_i", include_str!("../../assets/fonts/iso3098_i.lff")),
    ("iso3098", include_str!("../../assets/fonts/iso3098.lff")),
    ("iso", include_str!("../../assets/fonts/iso.lff")),
    ("italicc", include_str!("../../assets/fonts/italicc.lff")),
    ("italiccs", include_str!("../../assets/fonts/italiccs.lff")),
    ("italict", include_str!("../../assets/fonts/italict.lff")),
    ("kochigothic", include_str!("../../assets/fonts/kochigothic.lff")),
    ("kochimincho", include_str!("../../assets/fonts/kochimincho.lff")),
    ("kst32b", include_str!("../../assets/fonts/kst32b.lff")),
    ("lc_opengost-ar", include_str!("../../assets/fonts/lc_opengost-ar.lff")),
    ("lc_opengost-br", include_str!("../../assets/fonts/lc_opengost-br.lff")),
    ("opengosttypea-regular", include_str!("../../assets/fonts/opengosttypea-regular.lff")),
    ("opengosttypeb-regular", include_str!("../../assets/fonts/opengosttypeb-regular.lff")),
    ("romanc", include_str!("../../assets/fonts/romanc.lff")),
    ("romancs", include_str!("../../assets/fonts/romancs.lff")),
    ("romand", include_str!("../../assets/fonts/romand.lff")),
    ("romanp", include_str!("../../assets/fonts/romanp.lff")),
    ("romansi", include_str!("../../assets/fonts/romansi.lff")),
    ("romans", include_str!("../../assets/fonts/romans.lff")),
    ("romant", include_str!("../../assets/fonts/romant.lff")),
    ("scriptc", include_str!("../../assets/fonts/scriptc.lff")),
    ("scripts", include_str!("../../assets/fonts/scripts.lff")),
    ("simplex", include_str!("../../assets/fonts/simplex.lff")),
    ("standard", include_str!("../../assets/fonts/standard.lff")),
    ("syastro", include_str!("../../assets/fonts/syastro.lff")),
    ("symap", include_str!("../../assets/fonts/symap.lff")),
    ("symath", include_str!("../../assets/fonts/symath.lff")),
    ("symbol", include_str!("../../assets/fonts/symbol.lff")),
    ("symbol_misc1", include_str!("../../assets/fonts/symbol_misc1.lff")),
    ("symbol_misc2", include_str!("../../assets/fonts/symbol_misc2.lff")),
    ("symeteo", include_str!("../../assets/fonts/symeteo.lff")),
    ("symusic", include_str!("../../assets/fonts/symusic.lff")),
    ("unicode", include_str!("../../assets/fonts/unicode.lff")),
];

/// AutoCAD / DXF SHX font names → LFF stem. Names that already match a stem
/// (romans, italicc, scripts, symap, …) resolve directly and need no entry.
const ALIASES: &[(&str, &str)] = &[
    // AutoCAD/DXF SHX font names → the nearest LFF stem. Names that already
    // match a stem (romans, italicc, scripts, iso, symap, …) resolve directly
    // and need no entry. Unknown names fall back to `standard` (as LibreCAD's
    // `requestFont` does).
    ("TXT", "standard"),
    ("MONOTXT", "standard"),
    ("ISOCP", "iso"),
    ("ISOCP2", "iso"),
    ("ISOCP3", "iso"),
    ("ISOCPEUR", "iso"),
    ("ISOCT", "iso3098"),
    ("ISOCT2", "iso3098"),
    ("ISOCT3", "iso3098"),
    ("ISOCTEUR", "iso3098"),
    ("COMPLEX", "romanc"),
    ("ITALIC", "italicc"),
    ("GOTHICE", "gothgbt"),
    ("GOTHICG", "gothgrt"),
    ("GOTHICI", "gothitt"),
    ("CYRILLIC", "cyrillic_ii"),
    ("CYRILTLC", "cyrillic_ii"),
    ("GREEK", "greekc"),
    ("GOST", "opengosttypeb-regular"),
    ("GOSTA", "opengosttypea-regular"),
    ("GOSTB", "opengosttypeb-regular"),
    ("BIGFONT", "unicode"),
    ("EXTFONT", "unicode"),
];

// ── Public types ──────────────────────────────────────────────────────────

/// One glyph: a list of open 2-D polyline strokes in glyph units.
#[derive(Clone, Default)]
pub struct Glyph {
    pub strokes: Vec<Vec<[f32; 2]>>,
    /// Advance width in glyph units (rightmost X of all strokes).
    pub advance: f32,
}

/// A parsed LFF font.
#[derive(Clone)]
pub struct Font {
    pub name: String,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub line_spacing: f32,
    glyphs: HashMap<char, Glyph>,
}

impl Font {
    /// Look up a glyph by Unicode character.
    pub fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs.get(&c)
    }
}

// ── Registry ───────────────────────────────────────────────────────────────

static FONTS: OnceLock<HashMap<String, Font>> = OnceLock::new();
static WARNED_GLYPHS: OnceLock<Mutex<HashSet<(String, char)>>> = OnceLock::new();

fn warn_missing_glyph(font_name: &str, ch: char) {
    if ch.is_ascii() {
        return;
    }
    let set = WARNED_GLYPHS.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut guard) = set.lock() {
        if guard.insert((font_name.to_string(), ch)) {
            eprintln!(
                "lff: glyph U+{:04X} ('{}') not found in font '{font_name}'",
                ch as u32, ch
            );
        }
    }
}

fn fonts_map() -> &'static HashMap<String, Font> {
    FONTS.get_or_init(|| {
        let mut map = HashMap::new();
        // Register every font under its stem and its `# Name:` header.
        for (stem, src) in FONTS_SRC {
            let f = parse_lff(src);
            map.insert(stem.to_ascii_uppercase(), f.clone());
            map.entry(f.name.to_ascii_uppercase()).or_insert_with(|| f.clone());
        }
        // AutoCAD/DXF SHX names → the matching LFF stem.
        for (alias, stem) in ALIASES {
            if let Some(f) = map.get(&stem.to_ascii_uppercase()).cloned() {
                map.insert(alias.to_ascii_uppercase(), f);
            }
        }
        map
    })
}

/// Broad-coverage fallback font for glyphs missing from the selected family.
fn unicode_font() -> &'static Font {
    let map = fonts_map();
    map.get("UNICODE")
        .or_else(|| map.get("STANDARD"))
        .or_else(|| map.values().next())
        .expect("at least one LFF font must be embedded")
}

/// Return a font by name (case-insensitive). `.shx` suffix is stripped.
/// Falls back to Standard → Unicode → any embedded font.
pub fn get_font(name: &str) -> &'static Font {
    let map = fonts_map();
    let key = name.trim().to_ascii_uppercase();
    map.get(&key)
        .or_else(|| {
            // Strip a trailing ".SHX" / extension and retry.
            key.rsplit_once('.').and_then(|(stem, _)| map.get(stem))
        })
        // LibreCAD's `requestFont` falls back to `standard` for unknown names.
        .or_else(|| map.get("STANDARD"))
        .or_else(|| map.get("UNICODE"))
        .or_else(|| map.values().next())
        .expect("at least one LFF font must be embedded")
}

// ── Text tessellation ───────────────────────────────────────────────────────

/// Tessellate a text run into world-space 2-D strokes. Same semantics as the
/// former CXF engine; the `tracking` multiplier scales `letter_spacing`.
pub fn tessellate_text_ex(
    origin: [f32; 2],
    height: f32,
    rotation: f32,
    width_factor: f32,
    oblique_angle: f32,
    font_name: &str,
    text: &str,
) -> Vec<Vec<[f32; 2]>> {
    tessellate_text_run(
        origin,
        height,
        rotation,
        width_factor,
        oblique_angle,
        1.0,
        font_name,
        text,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn tessellate_text_run(
    origin: [f32; 2],
    height: f32,
    rotation: f32,
    width_factor: f32,
    oblique_angle: f32,
    tracking: f32,
    font_name: &str,
    text: &str,
) -> Vec<Vec<[f32; 2]>> {
    if text.is_empty() || height <= 0.0 {
        return vec![];
    }

    let font = get_font(font_name);
    let scale = height / 9.0;
    let wf = if width_factor < 0.0 {
        width_factor.clamp(-100.0, -0.01)
    } else {
        width_factor.clamp(0.01, 100.0)
    };
    let ob = oblique_angle.tan();
    let (cos_r, sin_r) = (rotation.cos(), rotation.sin());

    let xform = |gx: f32, gy: f32, cx: f32| -> [f32; 2] {
        let sx = (cx + gx) * scale * wf + gy * scale * ob;
        let sy = gy * scale;
        [
            origin[0] + sx * cos_r - sy * sin_r,
            origin[1] + sx * sin_r + sy * cos_r,
        ]
    };

    let mut out: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut cursor_x: f32 = 0.0;
    let mut underline: Option<f32> = None;
    let mut overline: Option<f32> = None;
    let mut strikethrough: Option<f32> = None;
    const UNDER_Y: f32 = -1.5;
    const OVER_Y: f32 = 10.5;
    const STRIKE_Y: f32 = 4.5;

    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek().copied() {
                Some('L') => {
                    chars.next();
                    if underline.is_none() {
                        underline = Some(cursor_x);
                    }
                    continue;
                }
                Some('l') => {
                    chars.next();
                    if let Some(s) = underline.take() {
                        out.push(vec![xform(s, UNDER_Y, 0.0), xform(cursor_x, UNDER_Y, 0.0)]);
                    }
                    continue;
                }
                Some('O') => {
                    chars.next();
                    if overline.is_none() {
                        overline = Some(cursor_x);
                    }
                    continue;
                }
                Some('o') => {
                    chars.next();
                    if let Some(s) = overline.take() {
                        out.push(vec![xform(s, OVER_Y, 0.0), xform(cursor_x, OVER_Y, 0.0)]);
                    }
                    continue;
                }
                Some('K') => {
                    chars.next();
                    if strikethrough.is_none() {
                        strikethrough = Some(cursor_x);
                    }
                    continue;
                }
                Some('k') => {
                    chars.next();
                    if let Some(s) = strikethrough.take() {
                        out.push(vec![
                            xform(s, STRIKE_Y, 0.0),
                            xform(cursor_x, STRIKE_Y, 0.0),
                        ]);
                    }
                    continue;
                }
                _ => {}
            }
        }

        // DXF %%x special-character sequences.
        let render_ch: char = if ch == '%' && chars.peek() == Some(&'%') {
            chars.next();
            match chars.peek().map(|c| c.to_ascii_lowercase()) {
                Some('d') => {
                    chars.next();
                    '°'
                }
                Some('p') => {
                    chars.next();
                    '±'
                }
                Some('c') => {
                    chars.next();
                    '⌀'
                }
                Some('%') => {
                    chars.next();
                    '%'
                }
                Some('u') => {
                    chars.next();
                    underline = match underline.take() {
                        Some(start) => {
                            out.push(vec![
                                xform(start, UNDER_Y, 0.0),
                                xform(cursor_x, UNDER_Y, 0.0),
                            ]);
                            None
                        }
                        None => Some(cursor_x),
                    };
                    continue;
                }
                Some('o') => {
                    chars.next();
                    overline = match overline.take() {
                        Some(start) => {
                            out.push(vec![xform(start, OVER_Y, 0.0), xform(cursor_x, OVER_Y, 0.0)]);
                            None
                        }
                        None => Some(cursor_x),
                    };
                    continue;
                }
                Some(d) if d.is_ascii_digit() => {
                    let mut digits = String::with_capacity(3);
                    for _ in 0..3 {
                        match chars.peek() {
                            Some(&c) if c.is_ascii_digit() => digits.push(chars.next().unwrap()),
                            _ => break,
                        }
                    }
                    if digits.len() == 3 {
                        match digits.parse::<u32>().ok().and_then(char::from_u32) {
                            Some(c) => c,
                            None => continue,
                        }
                    } else {
                        cursor_x += (6.0 + font.letter_spacing * tracking) * wf;
                        continue;
                    }
                }
                _ => continue,
            }
        } else {
            ch
        };

        if render_ch == ' ' {
            cursor_x += font.word_spacing;
            continue;
        }
        // Selected family first, then the broad Unicode fallback.
        let glyph = font
            .glyph(render_ch)
            .or_else(|| unicode_font().glyph(render_ch));
        match glyph {
            Some(glyph) => {
                for stroke in &glyph.strokes {
                    if stroke.len() < 2 {
                        continue;
                    }
                    out.push(stroke.iter().map(|&[gx, gy]| xform(gx, gy, cursor_x)).collect());
                }
                cursor_x += (glyph.advance + font.letter_spacing * tracking) * wf;
            }
            None => {
                warn_missing_glyph(font_name, render_ch);
                cursor_x += (6.0 + font.letter_spacing * tracking) * wf;
            }
        }
    }

    if let Some(start) = underline {
        out.push(vec![xform(start, UNDER_Y, 0.0), xform(cursor_x, UNDER_Y, 0.0)]);
    }
    if let Some(start) = overline {
        out.push(vec![xform(start, OVER_Y, 0.0), xform(cursor_x, OVER_Y, 0.0)]);
    }
    if let Some(start) = strikethrough {
        out.push(vec![xform(start, STRIKE_Y, 0.0), xform(cursor_x, STRIKE_Y, 0.0)]);
    }

    out
}

// ── Parser ────────────────────────────────────────────────────────────────

/// Intermediate glyph carrying unresolved `C<hex>` references.
#[derive(Default, Clone)]
struct RawGlyph {
    strokes: Vec<Vec<[f32; 2]>>,
    refs: Vec<char>,
}

fn parse_lff(src: &str) -> Font {
    let mut font = Font {
        name: String::from("Unknown"),
        letter_spacing: 3.0,
        word_spacing: 6.75,
        line_spacing: 1.0,
        glyphs: HashMap::new(),
    };

    let mut raw: HashMap<char, RawGlyph> = HashMap::new();
    let mut cur: Option<char> = None;
    let mut cur_glyph = RawGlyph::default();

    let flush = |cur: &mut Option<char>, g: &mut RawGlyph, raw: &mut HashMap<char, RawGlyph>| {
        if let Some(c) = cur.take() {
            raw.insert(c, std::mem::take(g));
        } else {
            *g = RawGlyph::default();
        }
    };

    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(rest) = t.strip_prefix('#') {
            if let Some(v) = rest.trim().strip_prefix("Name:") {
                font.name = v.trim().to_string();
            } else if let Some(v) = rest.trim().strip_prefix("LetterSpacing:") {
                if let Ok(x) = v.trim().parse() {
                    font.letter_spacing = x;
                }
            } else if let Some(v) = rest.trim().strip_prefix("WordSpacing:") {
                if let Ok(x) = v.trim().parse() {
                    font.word_spacing = x;
                }
            } else if let Some(v) = rest.trim().strip_prefix("LineSpacingFactor:") {
                if let Ok(x) = v.trim().parse() {
                    font.line_spacing = x;
                }
            }
            continue;
        }
        if t.starts_with('[') {
            flush(&mut cur, &mut cur_glyph, &mut raw);
            if let Some(end) = t.find(']') {
                let hex = t[1..end].trim();
                if let Some(c) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                    cur = Some(c);
                }
            }
            continue;
        }
        if cur.is_none() {
            continue;
        }
        // `C<hex>` — reference another glyph's strokes.
        if let Some(hex) = t.strip_prefix(['C', 'c']) {
            if hex.chars().all(|c| c.is_ascii_hexdigit()) && !hex.is_empty() {
                if let Some(c) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                    cur_glyph.refs.push(c);
                }
                continue;
            }
        }
        // Stroke polyline: `x,y;x,y;x,y,A<bulge>;…`
        if let Some(stroke) = parse_stroke_line(t) {
            if stroke.len() >= 2 {
                cur_glyph.strokes.push(stroke);
            }
        }
    }
    flush(&mut cur, &mut cur_glyph, &mut raw);

    // Resolve `C<hex>` references. Each pass folds in targets that are
    // themselves already reference-free; repeat so ref-to-ref chains settle.
    for _ in 0..4 {
        let keys: Vec<char> = raw.keys().copied().collect();
        let mut changed = false;
        for k in keys {
            let refs = raw.get(&k).map(|g| g.refs.clone()).unwrap_or_default();
            if refs.is_empty() {
                continue;
            }
            let mut add: Vec<Vec<[f32; 2]>> = Vec::new();
            let mut remaining: Vec<char> = Vec::new();
            for r in refs {
                match raw.get(&r) {
                    Some(rg) if rg.refs.is_empty() => add.extend(rg.strokes.iter().cloned()),
                    _ => remaining.push(r),
                }
            }
            if let Some(g) = raw.get_mut(&k) {
                g.strokes.extend(add);
                g.refs = remaining;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    for (c, g) in raw {
        let advance = g
            .strokes
            .iter()
            .flat_map(|s| s.iter())
            .map(|&[x, _]| x)
            .fold(0.0_f32, f32::max);
        font.glyphs.insert(
            c,
            Glyph {
                strokes: g.strokes,
                advance,
            },
        );
    }
    font
}

/// Parse one LFF stroke line into a polyline, tessellating bulge arcs.
///
/// A vertex's `A<bulge>` describes the arc from the PREVIOUS vertex to this
/// one (LibreCAD's `RS_Polyline::addVertex(v, bulge)` convention), so the
/// bulge curves the segment ending at the vertex that carries it.
fn parse_stroke_line(line: &str) -> Option<Vec<[f32; 2]>> {
    let mut pts: Vec<[f32; 2]> = Vec::new();
    for tok in line.split(';').filter(|s| !s.trim().is_empty()) {
        let parts: Vec<&str> = tok.split(',').map(|s| s.trim()).collect();
        if parts.len() < 2 {
            continue;
        }
        let x: f32 = parts[0].parse().ok()?;
        let y: f32 = parts[1].parse().ok()?;
        let p = [x, y];
        let bulge = if parts.len() >= 3 {
            parts[2]
                .strip_prefix(['A', 'a'])
                .and_then(|b| b.parse::<f32>().ok())
                .unwrap_or(0.0)
        } else {
            0.0
        };
        if pts.is_empty() || bulge.abs() < 1e-6 {
            pts.push(p);
        } else {
            let from = *pts.last().unwrap();
            for q in bulge_to_points(from, p, bulge) {
                pts.push(q);
            }
        }
    }
    if pts.is_empty() {
        None
    } else {
        Some(pts)
    }
}

/// Tessellate a DXF bulge arc from `p0` to `p1`. Returns the intermediate
/// points plus the end point (the start point is already in the polyline).
fn bulge_to_points(p0: [f32; 2], p1: [f32; 2], bulge: f32) -> Vec<[f32; 2]> {
    let dx = p1[0] - p0[0];
    let dy = p1[1] - p0[1];
    let chord = (dx * dx + dy * dy).sqrt();
    if bulge.abs() < 1e-6 || chord < 1e-9 {
        return vec![p1];
    }
    let theta = 4.0 * bulge.atan(); // signed included angle
    let radius = (chord * 0.5) / (theta * 0.5).sin(); // signed
    let mx = (p0[0] + p1[0]) * 0.5;
    let my = (p0[1] + p1[1]) * 0.5;
    let ux = dx / chord;
    let uy = dy / chord;
    // Left normal of the chord.
    let nx = -uy;
    let ny = ux;
    let off = radius * (theta * 0.5).cos();
    let cx = mx + nx * off;
    let cy = my + ny * off;
    let r = radius.abs();
    let a0 = (p0[1] - cy).atan2(p0[0] - cx);
    let n = ((theta.abs() / (std::f32::consts::FRAC_PI_8)).ceil() as usize).clamp(2, 64);
    let mut pts = Vec::with_capacity(n);
    for i in 1..=n {
        let a = a0 + theta * (i as f32 / n as f32);
        pts.push([cx + r * a.cos(), cy + r * a.sin()]);
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_parse_and_resolve() {
        // Straight-stroke glyph.
        let a = get_font("Standard").glyph('A').expect("A glyph");
        assert!(!a.strokes.is_empty() && a.advance > 0.0);
        // Bulge-arc glyph tessellates into a multi-point polyline.
        let zero = get_font("Standard").glyph('0').expect("0 glyph");
        assert!(zero.strokes.iter().any(|s| s.len() > 4));
        // Alias + fallback resolve to a real font.
        assert!(get_font("txt").glyph('A').is_some());
        assert!(get_font("RomanS").glyph('B').is_some());
        // Unicode fallback covers a non-ASCII letter via the renderer path.
        let strokes = tessellate_text_run([0.0, 0.0], 2.5, 0.0, 1.0, 0.0, 1.0, "Standard", "Aб");
        assert!(!strokes.is_empty());
        // The bulge belongs to the segment ENDING at the vertex (LibreCAD
        // convention): the standard/iso/unicode 'O' must come out as an
        // upright oval (taller than wide), not a sideways capsule.
        for name in ["standard", "iso", "unicode", "txt"] {
            let o = get_font(name).glyph('O').expect("O glyph");
            let (mut minx, mut miny, mut maxx, mut maxy) =
                (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for s in &o.strokes {
                for &[x, y] in s {
                    minx = minx.min(x);
                    miny = miny.min(y);
                    maxx = maxx.max(x);
                    maxy = maxy.max(y);
                }
            }
            let (w, h) = (maxx - minx, maxy - miny);
            assert!(h > w, "{name} O should be upright (h {h:.1} > w {w:.1})");
        }
    }
}

