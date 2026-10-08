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

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::{Mutex, OnceLock};

// ── Embedded fonts ─────────────────────────────────────────────────────────

/// Every LibreCAD LFF font, keyed by its file stem (lower-case). Registered
/// under the upper-cased stem and the font's `# Name:` header.
const FONTS_SRC: &[(&str, &str)] = &[
    ("cyrillic_ii", include_str!("../../../assets/fonts/cyrillic_ii.lff")),
    ("gothgbt", include_str!("../../../assets/fonts/gothgbt.lff")),
    ("gothgrt", include_str!("../../../assets/fonts/gothgrt.lff")),
    ("gothitt", include_str!("../../../assets/fonts/gothitt.lff")),
    ("greekc", include_str!("../../../assets/fonts/greekc.lff")),
    ("greeks", include_str!("../../../assets/fonts/greeks.lff")),
    ("iso3098", include_str!("../../../assets/fonts/iso3098.lff")),
    ("iso", include_str!("../../../assets/fonts/iso.lff")),
    ("italicc", include_str!("../../../assets/fonts/italicc.lff")),
    ("italict", include_str!("../../../assets/fonts/italict.lff")),
    ("ltypeshp", include_str!("../../../assets/fonts/ltypeshp.lff")),
    ("romanc", include_str!("../../../assets/fonts/romanc.lff")),
    ("romand", include_str!("../../../assets/fonts/romand.lff")),
    ("romans", include_str!("../../../assets/fonts/romans.lff")),
    ("romant", include_str!("../../../assets/fonts/romant.lff")),
    ("scriptc", include_str!("../../../assets/fonts/scriptc.lff")),
    ("scripts", include_str!("../../../assets/fonts/scripts.lff")),
    ("simplex", include_str!("../../../assets/fonts/simplex.lff")),
    ("standard", include_str!("../../../assets/fonts/standard.lff")),
    ("syastro", include_str!("../../../assets/fonts/syastro.lff")),
    ("symap", include_str!("../../../assets/fonts/symap.lff")),
    ("symath", include_str!("../../../assets/fonts/symath.lff")),
    ("symbol", include_str!("../../../assets/fonts/symbol.lff")),
    ("symeteo", include_str!("../../../assets/fonts/symeteo.lff")),
    ("symusic", include_str!("../../../assets/fonts/symusic.lff")),
    ("unicode", include_str!("../../../assets/fonts/unicode.lff")),
    // Geometric-tolerance symbols. Registered like any other font and
    // keyed by the letter its `\Fgdt;X` escape carries, so a switch to it
    // anywhere — a tolerance frame, an MTEXT, a text style literally named
    // "gdt.shx" — resolves through the ordinary name lookup and needs no
    // special case. It holds no Latin text: a letter IS a symbol here.
    ("gdt", include_str!("../../../assets/fonts/gdt.lff")),
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
    ("BIGFONT", "unicode"),
    ("EXTFONT", "unicode"),
];

/// The space advance of the reference's stroke fonts, in glyph units (cap
/// height 9), measured from their text extents: the SHX names these LFF
/// fonts stand in for keep the reference's word gaps.
const SHX_SPACE: &[(&str, f32)] = &[
    ("TXT", 9.0),
    ("MONOTXT", 9.0),
    ("SIMPLEX", 9.0 * 19.0 / 21.0),
    ("ROMANS", 9.0),
    ("ROMAND", 9.0),
    ("ROMANC", 9.0),
    ("COMPLEX", 9.0),
    ("ITALIC", 9.0),
    ("GDT", 9.0),
    ("ISOCP", 3.6),
];

/// The pen advance of each printable ASCII character of the reference's
/// stroke fonts, in glyph units (cap height 9), measured from their text
/// extents: the SHX names these LFF fonts stand in for keep the reference's
/// character widths, so lines measure, wrap and align like the reference.
const SHX_ADVANCE: &[(&str, &[(u32, f32)])] = &[
    (
        "TXT",
        &[
            (33, 3.0000), (34, 6.0000), (35, 9.0000), (36, 9.0000), (38, 9.0000), (39, 4.5000),
            (40, 6.0000), (41, 6.0000), (42, 9.0000), (43, 9.0000), (44, 4.5000), (45, 9.0000),
            (46, 3.0000), (47, 9.0000), (48, 7.5000), (49, 6.0000), (50, 9.0000), (51, 9.0000),
            (52, 9.0000), (53, 9.0000), (54, 9.0000), (55, 9.0000), (56, 9.0000), (57, 9.0000),
            (58, 3.0000), (59, 4.5000), (60, 7.5000), (61, 9.0000), (62, 7.5000), (63, 7.5000),
            (64, 9.0000), (65, 9.0000), (66, 9.0000), (67, 9.0000), (68, 9.0000), (69, 9.0000),
            (70, 9.0000), (71, 9.0000), (72, 9.0000), (73, 6.0000), (74, 9.0000), (75, 9.0000),
            (76, 9.0000), (77, 9.0000), (78, 9.0000), (79, 9.0000), (80, 9.0000), (81, 9.0000),
            (82, 9.0000), (83, 9.0000), (84, 9.0000), (85, 9.0000), (86, 12.0000), (87, 12.0000),
            (88, 9.0000), (89, 9.0000), (90, 9.0000), (91, 6.0000), (92, 9.0000), (93, 6.0000),
            (94, 9.0000), (95, 9.0000), (96, 4.5000), (97, 9.0000), (98, 9.0000), (99, 9.0000),
            (100, 9.0000), (101, 9.0000), (102, 9.0000), (103, 9.0000), (104, 9.0000), (105, 3.0000),
            (106, 7.5000), (107, 9.0000), (108, 4.5000), (109, 9.0000), (110, 9.0000), (111, 9.0000),
            (112, 9.0000), (113, 9.0000), (114, 9.0000), (115, 9.0000), (116, 9.0000), (117, 9.0000),
            (118, 9.0000), (119, 9.0000), (120, 9.0000), (121, 9.0000), (122, 9.0000), (123, 6.0000),
            (124, 3.0000), (125, 6.0000), (126, 9.0000),
        ],
    ),
    (
        "SIMPLEX",
        &[
            (33, 5.5714), (34, 6.4286), (35, 8.1429), (36, 8.5714), (38, 11.1429), (39, 3.8571),
            (40, 6.0000), (41, 6.0000), (42, 6.8571), (43, 11.1429), (44, 4.2857), (45, 11.1429),
            (46, 4.2857), (47, 9.4286), (48, 8.5714), (49, 6.8571), (50, 8.5714), (51, 8.5714),
            (52, 8.5714), (53, 8.5714), (54, 8.5714), (55, 8.5714), (56, 8.5714), (57, 8.5714),
            (58, 6.4286), (59, 6.0000), (60, 8.5714), (61, 11.1429), (62, 8.5714), (63, 7.7143),
            (64, 10.2857), (65, 9.4286), (66, 9.0000), (67, 9.0000), (68, 9.0000), (69, 8.1429),
            (70, 7.7143), (71, 9.0000), (72, 9.4286), (73, 3.4286), (74, 6.8571), (75, 9.0000),
            (76, 7.2857), (77, 10.2857), (78, 9.4286), (79, 9.4286), (80, 9.0000), (81, 9.4286),
            (82, 9.0000), (83, 8.5714), (84, 7.7143), (85, 9.4286), (86, 8.5714), (87, 10.2857),
            (88, 8.5714), (89, 8.5714), (90, 8.5714), (91, 6.4286), (92, 9.4286), (93, 5.1429),
            (94, 8.5714), (95, 10.2857), (96, 3.8571), (97, 8.1429), (98, 8.1429), (99, 7.7143),
            (100, 8.1429), (101, 7.7143), (102, 5.1429), (103, 8.1429), (104, 8.1429), (105, 3.4286),
            (106, 4.2857), (107, 7.2857), (108, 3.4286), (109, 12.8571), (110, 8.1429), (111, 8.1429),
            (112, 8.1429), (113, 8.1429), (114, 5.5714), (115, 7.2857), (116, 6.0000), (117, 8.1429),
            (118, 6.8571), (119, 9.4286), (120, 7.2857), (121, 6.8571), (122, 7.2857), (123, 5.1429),
            (124, 3.0000), (125, 4.7143), (126, 11.1429),
        ],
    ),
    (
        "ROMANS",
        &[
            (33, 4.2857), (34, 6.0000), (35, 9.0000), (36, 8.5714), (38, 11.1429), (39, 4.2857),
            (40, 6.0000), (41, 6.0000), (42, 6.8571), (43, 11.1429), (44, 4.2857), (45, 11.1429),
            (46, 4.2857), (47, 9.4286), (48, 8.5714), (49, 8.5714), (50, 8.5714), (51, 8.5714),
            (52, 8.5714), (53, 8.5714), (54, 8.5714), (55, 8.5714), (56, 8.5714), (57, 8.5714),
            (58, 4.2857), (59, 4.2857), (60, 10.2857), (61, 11.1429), (62, 10.2857), (63, 7.7143),
            (64, 11.5714), (65, 7.7143), (66, 9.0000), (67, 9.0000), (68, 9.0000), (69, 8.1429),
            (70, 7.7143), (71, 9.0000), (72, 9.4286), (73, 3.4286), (74, 6.8571), (75, 9.0000),
            (76, 7.2857), (77, 10.2857), (78, 9.4286), (79, 9.4286), (80, 9.0000), (81, 9.4286),
            (82, 9.0000), (83, 8.5714), (84, 6.8571), (85, 9.4286), (86, 7.7143), (87, 10.2857),
            (88, 8.5714), (89, 7.7143), (90, 8.5714), (91, 6.0000), (92, 9.4286), (93, 5.5714),
            (94, 9.4286), (95, 10.2857), (96, 5.1429), (97, 8.1429), (98, 8.1429), (99, 7.7143),
            (100, 8.1429), (101, 7.7143), (102, 5.1429), (103, 8.1429), (104, 8.1429), (105, 3.4286),
            (106, 4.2857), (107, 7.2857), (108, 3.4286), (109, 12.8571), (110, 8.1429), (111, 8.1429),
            (112, 8.1429), (113, 8.1429), (114, 5.5714), (115, 7.2857), (116, 5.1429), (117, 8.1429),
            (118, 6.8571), (119, 9.4286), (120, 7.2857), (121, 6.8571), (122, 7.2857), (123, 6.0000),
            (124, 3.4286), (125, 6.0000), (126, 11.1429),
        ],
    ),
];

// ── Public types ──────────────────────────────────────────────────────────

/// One glyph: a list of open 2-D polyline strokes in glyph units.
#[derive(Clone, Default)]
pub struct Glyph {
    pub strokes: Vec<Vec<[f32; 2]>>,
    /// Advance width in glyph units (rightmost X of all strokes).
    pub advance: f32,
    pub fill_tris: Vec<[f32; 2]>,
}

/// A parsed LFF font.
#[derive(Clone)]
pub struct Font {
    pub name: String,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub line_spacing: f32,
    glyphs: HashMap<char, Glyph>,
    /// Named shapes — blocks whose `[hex] LABEL` label is a word rather than
    /// the single codepoint character (used by `ltypeshp` for complex
    /// linetype shapes, keyed by name since codepoints collide).
    shapes: HashMap<String, Glyph>,
}

impl Font {
    /// Look up a glyph by Unicode character.
    pub fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs.get(&c)
    }
    /// Look up a named shape (case-insensitive).
    pub fn shape(&self, name: &str) -> Option<&Glyph> {
        self.shapes.get(&name.to_ascii_uppercase())
    }
}

/// Look up a complex-linetype shape by name in the `ltypeshp` font.
pub fn shape(name: &str) -> Option<&'static Glyph> {
    fonts_map().get("LTYPESHP").and_then(|f| f.shape(name))
}

// ── Registry ───────────────────────────────────────────────────────────────

static FONTS: OnceLock<HashMap<String, Font>> = OnceLock::new();
static WARNED_GLYPHS: OnceLock<Mutex<HashSet<(String, char)>>> = OnceLock::new();

fn warn_missing_glyph(font_name: &str, ch: char) {
    if ch.is_ascii() {
        return;
    }
    let set = WARNED_GLYPHS.get_or_init(|| Mutex::new(HashSet::default()));
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
        let mut map = HashMap::default();
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
        // Those names keep the reference font's space width and character
        // advances (a glyph's advance plus the letter spacing).
        for (name, word) in SHX_SPACE {
            if let Some(f) = map.get_mut(*name) {
                f.word_spacing = *word;
            }
        }
        for (name, advances) in SHX_ADVANCE {
            if let Some(f) = map.get_mut(*name) {
                let spacing = f.letter_spacing;
                for (code, advance) in advances.iter() {
                    if let Some(glyph) = char::from_u32(*code).and_then(|c| f.glyphs.get_mut(&c)) {
                        glyph.advance = (advance - spacing).max(0.0);
                    }
                }
            }
        }
        map
    })
}

/// Whether `name` resolves to an actual embedded LFF font (after stripping a
/// trailing extension) — i.e. `get_font` would return a real match rather than
/// the `STANDARD` fallback. Lets the font dispatcher prefer a system TrueType
/// face only for names that are *not* one of our stroke fonts.
pub fn is_builtin(name: &str) -> bool {
    let map = fonts_map();
    let key = name.trim().to_ascii_uppercase();
    map.contains_key(&key)
        || key
            .rsplit_once('.')
            .is_some_and(|(stem, _)| map.contains_key(stem))
}

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

// ── Run tokenizer ────────────────────────────────────────────────────────────

/// Decoration line a token toggles.
#[derive(Clone, Copy)]
pub(crate) enum Deco {
    Under,
    Over,
    Strike,
}

/// How a decoration token changes state. `\L`/`\O`/`\K` turn on (idempotent),
/// `\l`/`\o`/`\k` turn off; `%%u`/`%%o` flip.
#[derive(Clone, Copy)]
pub(crate) enum Op {
    On,
    Off,
    Toggle,
}

/// One unit of a text run after inline-code resolution. Shared by the stroke
/// (LFF) and shaped (TTF) renderers so both interpret the DXF inline grammar
/// — `\L…\l` decorations and `%%` specials — identically.
pub(crate) enum Tok {
    /// A renderable character (DXF `%%d`/`%%p`/`%%c`/`%%nnn` already resolved).
    Glyph(char),
    /// A literal space.
    Space,
    /// A `%%nnn` escape with fewer than three digits — advances like a missing
    /// glyph without drawing anything.
    Missing,
    /// A decoration toggle.
    Deco(Deco, Op),
}

/// Resolve a run's inline codes into a flat token stream. Mirrors the original
/// inline parser in `tessellate_text_run` exactly (verified by the golden test).
pub(crate) fn tokenize_run(text: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        // A direction mark only steers shaping; it draws nothing.
        if ch == crate::scene::text::ttf_glyph::LRM {
            continue;
        }
        if ch == '\\' {
            match chars.peek().copied() {
                Some('L') => {
                    chars.next();
                    toks.push(Tok::Deco(Deco::Under, Op::On));
                    continue;
                }
                Some('l') => {
                    chars.next();
                    toks.push(Tok::Deco(Deco::Under, Op::Off));
                    continue;
                }
                Some('O') => {
                    chars.next();
                    toks.push(Tok::Deco(Deco::Over, Op::On));
                    continue;
                }
                Some('o') => {
                    chars.next();
                    toks.push(Tok::Deco(Deco::Over, Op::Off));
                    continue;
                }
                Some('K') => {
                    chars.next();
                    toks.push(Tok::Deco(Deco::Strike, Op::On));
                    continue;
                }
                Some('k') => {
                    chars.next();
                    toks.push(Tok::Deco(Deco::Strike, Op::Off));
                    continue;
                }
                _ => {}
            }
        }

        if ch == '%' && chars.peek() == Some(&'%') {
            chars.next();
            match chars.peek().map(|c| c.to_ascii_lowercase()) {
                Some('d') => {
                    chars.next();
                    toks.push(Tok::Glyph('°'));
                }
                Some('p') => {
                    chars.next();
                    toks.push(Tok::Glyph('±'));
                }
                Some('c') => {
                    chars.next();
                    toks.push(Tok::Glyph('⌀'));
                }
                Some('%') => {
                    chars.next();
                    toks.push(Tok::Glyph('%'));
                }
                Some('u') => {
                    chars.next();
                    toks.push(Tok::Deco(Deco::Under, Op::Toggle));
                }
                Some('o') => {
                    chars.next();
                    toks.push(Tok::Deco(Deco::Over, Op::Toggle));
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
                        if let Some(c) = digits.parse::<u32>().ok().and_then(char::from_u32) {
                            toks.push(Tok::Glyph(c));
                        }
                    } else {
                        toks.push(Tok::Missing);
                    }
                }
                _ => {}
            }
            continue;
        }

        toks.push(if ch == ' ' { Tok::Space } else { Tok::Glyph(ch) });
    }
    toks
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
) -> (Vec<Vec<[f32; 2]>>, Vec<[f32; 2]>) {
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
) -> (Vec<Vec<[f32; 2]>>, Vec<[f32; 2]>) {
    if text.is_empty() || height <= 0.0 {
        return (vec![], vec![]);
    }

    let face = crate::scene::text::font_face::Face::resolve(font_name);
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
    let mut fill_tris: Vec<[f32; 2]> = Vec::new();
    let mut cursor_x: f32 = 0.0;
    let mut underline: Option<f32> = None;
    let mut overline: Option<f32> = None;
    let mut strikethrough: Option<f32> = None;
    const UNDER_Y: f32 = -1.5;
    const OVER_Y: f32 = 10.5;
    const STRIKE_Y: f32 = 4.5;

    let toks = tokenize_run(text);
    let ttf_family = face.ttf_family();
    let shaping_family = ttf_family.or_else(|| {
        crate::scene::text::web_font::requires_shaping(text)
            .then(|| crate::scene::text::web_font::primary_script().family())
    });

    // Render a stroke directly (LFF glyph) or per shaped contour (TTF), applying
    // the run transform at the current pen position.
    let emit_glyph = |out: &mut Vec<Vec<[f32; 2]>>, strokes: &[Vec<[f32; 2]>], cx: f32| {
        for stroke in strokes {
            if stroke.len() < 2 {
                continue;
            }
            out.push(stroke.iter().map(|&[gx, gy]| xform(gx, gy, cx)).collect());
        }
    };

    let emit_fill = |fill_tris: &mut Vec<[f32; 2]>, tris: &[[f32; 2]], cx: f32| {
        for &v in tris {
            fill_tris.push(xform(v[0], v[1], cx));
        }
    };

    // Flush a buffered TTF segment: shape it, emit the positioned glyph
    // contours, and advance the pen by the shaped run width. Falls back to
    // per-glyph outlines if shaping is unavailable.
    let flush_ttf = |seg: &mut String, cursor_x: &mut f32, out: &mut Vec<Vec<[f32; 2]>>, fill_tris: &mut Vec<[f32; 2]>| {
        if seg.is_empty() {
            return;
        }
        let family = shaping_family.unwrap_or("");
        if let Some(run) = crate::scene::text::ttf_glyph::shape_run(family, seg) {
            for g in &run.glyphs {
                emit_glyph(out, &g.strokes, *cursor_x);
                emit_fill(fill_tris, &g.fill_tris, *cursor_x);
            }
            // Advance the pen in unscaled glyph units — the `xform` scales the
            // pen position by `wf`, so multiplying the advance here too would
            // apply the width factor twice (and reverse a backward run's word
            // order onto the wrong side). Matches `text_local_bounds`.
            *cursor_x += run.advance;
        } else {
            for ch in seg.chars() {
                match face.glyph(ch) {
                    Some(glyph) => {
                        emit_glyph(out, &glyph.strokes, *cursor_x);
                        emit_fill(fill_tris, &glyph.fill_tris, *cursor_x);
                        *cursor_x += glyph.advance + face.spacing_after(ch) * tracking;
                    }
                    None => {
                        warn_missing_glyph(font_name, ch);
                        *cursor_x += 6.0 + face.letter_spacing() * tracking;
                    }
                }
            }
        }
        seg.clear();
    };

    let mut seg = String::new();
    for tok in &toks {
        // TTF shaping batches consecutive glyphs; any break (space, missing,
        // decoration toggle, end of run) flushes the buffer first so pen
        // positions stay correct for decorations.
        if shaping_family.is_some() && !matches!(tok, Tok::Glyph(_) | Tok::Space) {
            flush_ttf(&mut seg, &mut cursor_x, &mut out, &mut fill_tris);
        }
        match tok {
            Tok::Glyph(c) => {
                if shaping_family.is_some() {
                    seg.push(*c);
                } else {
                    match face.glyph(*c) {
                        Some(glyph) => {
                            emit_glyph(&mut out, &glyph.strokes, cursor_x);
                            emit_fill(&mut fill_tris, &glyph.fill_tris, cursor_x);
                            cursor_x += glyph.advance + face.spacing_after(*c) * tracking;
                        }
                        None => {
                            warn_missing_glyph(font_name, *c);
                            cursor_x += 6.0 + face.letter_spacing() * tracking;
                        }
                    }
                }
            }
            Tok::Space if shaping_family.is_some() => seg.push(' '),
            Tok::Space => cursor_x += face.word_spacing(),
            Tok::Missing => cursor_x += 6.0 + face.letter_spacing() * tracking,
            Tok::Deco(deco, op) => {
                let (slot, y) = match deco {
                    Deco::Under => (&mut underline, UNDER_Y),
                    Deco::Over => (&mut overline, OVER_Y),
                    Deco::Strike => (&mut strikethrough, STRIKE_Y),
                };
                match op {
                    Op::On => {
                        if slot.is_none() {
                            *slot = Some(cursor_x);
                        }
                    }
                    Op::Off => {
                        if let Some(s) = slot.take() {
                            out.push(vec![xform(s, y, 0.0), xform(cursor_x, y, 0.0)]);
                        }
                    }
                    Op::Toggle => {
                        *slot = match slot.take() {
                            Some(s) => {
                                out.push(vec![xform(s, y, 0.0), xform(cursor_x, y, 0.0)]);
                                None
                            }
                            None => Some(cursor_x),
                        };
                    }
                }
            }
        }
    }
    if shaping_family.is_some() {
        flush_ttf(&mut seg, &mut cursor_x, &mut out, &mut fill_tris);
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

    (out, fill_tris)
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
        glyphs: HashMap::default(),
        shapes: HashMap::default(),
    };

    let mut raw: HashMap<char, RawGlyph> = HashMap::default();
    let mut raw_shapes: HashMap<String, RawGlyph> = HashMap::default();
    let mut cur: Option<char> = None;
    let mut cur_name: Option<String> = None;
    let mut cur_glyph = RawGlyph::default();

    // Route the just-finished block to the glyph map (by char) or, when it
    // carried a shape name, to the shape map (by name).
    macro_rules! flush {
        () => {{
            if let Some(c) = cur.take() {
                raw.insert(c, std::mem::take(&mut cur_glyph));
            } else if let Some(n) = cur_name.take() {
                raw_shapes.insert(n, std::mem::take(&mut cur_glyph));
            } else {
                // Discard any strokes seen before the first header.
                let _ = std::mem::take(&mut cur_glyph);
            }
        }};
    }

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
            flush!();
            if let Some(end) = t.find(']') {
                let hex = t[1..end].trim();
                let label = t[end + 1..].trim();
                let cp = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
                // A 0/1-char label is a normal glyph (keyed by codepoint); a
                // word label (BOX, CIRC1, …) is a named shape.
                if label.chars().count() > 1 {
                    cur_name = Some(label.to_ascii_uppercase());
                } else {
                    cur = cp;
                }
            }
            continue;
        }
        if cur.is_none() && cur_name.is_none() {
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
    flush!();

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

    let advance_of = |strokes: &[Vec<[f32; 2]>]| -> f32 {
        strokes
            .iter()
            .flat_map(|s| s.iter())
            .map(|&[x, _]| x)
            .fold(0.0_f32, f32::max)
    };
    for (c, g) in raw {
        let advance = advance_of(&g.strokes);
        font.glyphs.insert(c, Glyph { strokes: g.strokes, advance, fill_tris: Vec::new() });
    }
    for (n, g) in raw_shapes {
        let advance = advance_of(&g.strokes);
        font.shapes.insert(n, Glyph { strokes: g.strokes, advance, fill_tris: Vec::new() });
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
    /// Locks the LFF `tessellate_text_run` output (segment count, vertex count,
    /// coordinate checksum) so the shared-tokenizer refactor that adds the TTF
    /// shaping path cannot silently change stroke-font rendering.
    #[test]
    fn lff_tessellation_golden() {
        let cases: &[(&str, usize, usize, f64)] = &[
            // "txt" keeps the advances of the font it stands in for.
            ("ABC 123", 8, 80, 3189.9030),
            ("A\\LB\\lC", 6, 43, 842.2172),
            ("10%%d Ö", 6, 60, 1627.2223),
            ("Hello, World!", 15, 113, 6029.0805),
        ];
        for &(t, segs, verts, sum_ref) in cases {
            let (st, _) = tessellate_text_ex([0.0, 0.0], 10.0, 0.0, 1.0, 0.0, "txt", t);
            let nv: usize = st.iter().map(|s| s.len()).sum();
            let sum: f64 = st
                .iter()
                .flatten()
                .map(|p| p[0] as f64 + p[1] as f64)
                .sum();
            assert_eq!(st.len(), segs, "segment count drift for {t:?}");
            assert_eq!(nv, verts, "vertex count drift for {t:?}");
            assert!(
                (sum - sum_ref).abs() < 0.01,
                "coordinate checksum drift for {t:?}: {sum:.4} vs {sum_ref:.4}"
            );
        }
    }

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
        // After curation, every embedded stem and AutoCAD SHX alias must still
        // resolve to a glyph-bearing font (no alias points at a deleted file).
        for name in [
            "MONOTXT", "ISOCP", "ISOCP3", "ISOCT", "ISOCT3", "COMPLEX", "ITALIC", "GOTHICE",
            "GOTHICG", "GOTHICI", "CYRILLIC", "GREEK", "BIGFONT", "Simplex", "ScriptC", "Symbol",
        ] {
            assert!(
                get_font(name).glyph('A').is_some() || is_builtin(name),
                "alias/stem {name} failed to resolve"
            );
        }
        // A name from a deleted LibreCAD-only font is no longer a builtin and
        // falls back to Standard rather than panicking.
        assert!(!is_builtin("amiri-regular"));
        assert!(get_font("kochigothic").glyph('A').is_some());
        // Unicode fallback covers a non-ASCII letter via the renderer path.
        let (strokes, _) = tessellate_text_run([0.0, 0.0], 2.5, 0.0, 1.0, 0.0, 1.0, "Standard", "Aб");
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
        // Turkish letters absent from simplex/unicode still render via the
        // iso3098 fallback (ı/U+0131 is the one exception iso3098 lacks).
        for ch in ['Ğ', 'ş', 'İ', 'Ş', 'ğ'] {
            let (s, _) = tessellate_text_run(
                [0.0, 0.0],
                2.5,
                0.0,
                1.0,
                0.0,
                1.0,
                "simplex",
                &ch.to_string(),
            );
            assert!(!s.is_empty(), "Turkish '{ch}' should render via fallback");
        }
        // Complex-linetype shapes load by name (codepoints collide, so they
        // must be keyed by label).
        for sh in ["BOX", "CIRC1", "ZIG", "TRACK1"] {
            let g = shape(sh).unwrap_or_else(|| panic!("shape {sh} missing"));
            assert!(!g.strokes.is_empty(), "shape {sh} has no strokes");
        }
    }
}
