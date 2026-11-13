// System TrueType/OpenType font discovery.
//
// Wraps a `fontdb` database loaded with the user's installed system fonts so
// the rest of the app can (a) list available font families for the text-style
// picker and (b) borrow a face's raw bytes to extract glyph outlines (see the
// TTF glyph engine). LFF stroke fonts stay separate — this is purely the
// TrueType side of the renderer.
//
// The web build has no installed fonts to scan (`load_system_fonts` finds
// nothing on wasm), so the database is instead populated at runtime with the
// Noto subsets `web_font` fetches (#141): each completed fetch is registered
// via [`register_web_font`], and a static alias table maps the common
// office/CAD families (Arial, Cambria, …) onto that Noto set so styles naming
// them resolve to filled TTF glyphs instead of falling back to LFF strokes.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
struct FileFace {
    id: fontdb::ID,
    family: String,
}

struct SysFonts {
    db: Mutex<fontdb::Database>,
    /// Sorted, de-duplicated family names for the picker and name resolution.
    families: Mutex<Vec<String>>,
    /// Installed face file name (lower-case) → its exact face and family.
    /// A drawing references a TrueType style by *file* name, so this is what
    /// makes such a style resolve — see [`family_for_reference`]. Files a host
    /// registers at run time join it ([`register_font_file`]). Retaining the ID
    /// also lets ETRANSMIT distinguish two faces that share a family.
    by_file: Mutex<HashMap<String, FileFace>>,
    /// Drawing-relative resolved path → exact face, preventing preserved
    /// same-named fonts in different folders from replacing one another.
    #[cfg(not(target_arch = "wasm32"))]
    by_path: Mutex<HashMap<std::path::PathBuf, FileFace>>,
}

static FONTS: OnceLock<SysFonts> = OnceLock::new();

fn fonts() -> &'static SysFonts {
    FONTS.get_or_init(|| {
        // No system fonts to scan on the web — the database is filled from the
        // fetched Noto subsets instead (see `register_web_font`).
        #[cfg(not(target_arch = "wasm32"))]
        let db = {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            // TrueType files dropped into the Fonts folder resolve like
            // installed ones, as the folder's SHX fonts already do (#925).
            for dir in crate::io::font_repo::fonts_dir()
                .into_iter()
                .chain(crate::io::font_repo::support_folder())
            {
                db.load_fonts_dir(dir);
            }
            db
        };
        #[cfg(target_arch = "wasm32")]
        let db = fontdb::Database::new();

        let mut by_file: HashMap<String, FileFace> = HashMap::new();
        for face in db.faces() {
            let Some((family, _)) = face.families.first() else {
                continue;
            };
            // Remember every face file: a DWG style names the file, and the
            // family inside it is usually a different string. A bold or italic
            // file (`GOTHICB.TTF`) names that face of its family.
            if let fontdb::Source::File(path) = &face.source {
                if let Some(name) = path.file_name() {
                    by_file
                        .entry(name.to_string_lossy().to_ascii_lowercase())
                        .or_insert_with(|| FileFace {
                            id: face.id,
                            family: face_family(face, family),
                        });
                }
            }
        }
        let families = family_names(&db);
        SysFonts {
            db: Mutex::new(db),
            families: Mutex::new(families),
            by_file: Mutex::new(by_file),
            #[cfg(not(target_arch = "wasm32"))]
            by_path: Mutex::new(HashMap::new()),
        }
    })
}

/// `family` naming `face` itself: with the [`STYLE_SEP`] style of a bold or
/// italic face.
fn face_family(face: &fontdb::FaceInfo, family: &str) -> String {
    let b = if face.weight.0 >= fontdb::Weight::SEMIBOLD.0 {
        "b"
    } else {
        ""
    };
    let i = if face.style != fontdb::Style::Normal {
        "i"
    } else {
        ""
    };
    if b.is_empty() && i.is_empty() {
        family.to_string()
    } else {
        format!("{family}{STYLE_SEP}{b}{i}")
    }
}

/// Sorted, de-duplicated family names of every face currently in `db`.
fn family_names(db: &fontdb::Database) -> Vec<String> {
    let mut families: Vec<String> = db
        .faces()
        .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
        .collect();
    families.sort_by_key(|n| n.to_lowercase());
    families.dedup();
    families
}

/// All known system font families, sorted case-insensitively, de-duped. On the
/// web this is the fetched Noto set, so it grows as fetches complete.
pub fn families() -> Vec<String> {
    fonts().families.lock().unwrap().clone()
}

/// Sans families aliased to the Noto stand-in on the web.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
static SANS_ALIASES: &[&str] = &[
    "Arial",
    "Calibri",
    "DejaVu Sans",
    "Helvetica",
    "Liberation Sans",
    "Segoe UI",
    "Tahoma",
    "Verdana",
];

/// Serif families aliased on the web; they prefer Noto Serif when such a
/// subset is ever fetched and otherwise degrade to Noto Sans.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
static SERIF_ALIASES: &[&str] = &[
    "Book Antiqua",
    "Cambria",
    "Constantia",
    "DejaVu Serif",
    "Garamond",
    "Georgia",
    "Liberation Serif",
    "Palatino Linotype",
    "Times New Roman",
];

/// Preferred stand-in families for an alias request (lowercase), in order.
/// `None` when the name is not a known alias. Pure table lookup, so it is unit
/// tested on every platform even though only the web resolution path uses it.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn alias_targets(family_lower: &str) -> Option<&'static [&'static str]> {
    const SANS: &[&str] = &["Noto Sans"];
    const SERIF: &[&str] = &["Noto Serif", "Noto Sans"];
    if SANS_ALIASES
        .iter()
        .any(|alias| alias.eq_ignore_ascii_case(family_lower))
    {
        Some(SANS)
    } else if SERIF_ALIASES
        .iter()
        .any(|alias| alias.eq_ignore_ascii_case(family_lower))
    {
        Some(SERIF)
    } else {
        None
    }
}

/// First alias stand-in that `known` reports as available. `known` receives
/// candidate family names; on the live path it scans the registered families.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn alias_resolution(family_lower: &str, known: impl Fn(&str) -> bool) -> Option<&'static str> {
    alias_targets(family_lower)?
        .iter()
        .copied()
        .find(|t| known(t))
}

/// Append the alias names to a family list when the sans stand-in is present
/// (every fetched subset carries the Latin face, so one successful fetch makes
/// all aliases resolvable). Pure, so the picker behaviour is unit testable.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn merge_web_aliases(families: &mut Vec<String>) {
    if !families.iter().any(|f| f.eq_ignore_ascii_case("Noto Sans")) {
        return;
    }
    families.extend(
        SANS_ALIASES
            .iter()
            .chain(SERIF_ALIASES.iter())
            .map(|name| name.to_string()),
    );
    families.sort_by_key(|n| n.to_lowercase());
    families.dedup();
}

/// Families offered by the text-style picker's "System fonts" list: installed
/// fonts natively; on the web the fetched Noto families plus the common
/// office/CAD family names that stand in via them.
pub fn picker_families() -> Vec<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let mut families = families();
        merge_web_aliases(&mut families);
        families
    }
    #[cfg(not(target_arch = "wasm32"))]
    families()
}

fn canonical_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Resolve a requested family name to the canonical installed system family name (with exact case).
///
/// Memoised: `resolve_font` calls this once per word on the MTEXT
/// measure hot path for inline-`\f` TTF runs, and `Face::resolve` re-runs it
/// immediately after — the underlying fontdb query plus linear family scans are
/// not free. The cache keys on the raw request string. Results are stable for
/// the process lifetime on desktop; on the web the cache is cleared whenever a
/// fetched Noto subset joins the database, because earlier `None` resolutions
/// (font still in flight) must not stick.
pub fn canonical_family_name(family: &str) -> Option<String> {
    // A styled name keeps its style on the canonical family.
    if let Some((base, style)) = family.split_once(STYLE_SEP) {
        return canonical_family_name(base).map(|c| format!("{c}{STYLE_SEP}{style}"));
    }
    if let Some(hit) = canonical_cache().lock().unwrap().get(family) {
        return hit.clone();
    }
    let resolved = canonical_family_name_uncached(family);
    canonical_cache()
        .lock()
        .unwrap()
        .insert(family.to_string(), resolved.clone());
    resolved
}

fn canonical_family_name_uncached(family: &str) -> Option<String> {
    // Snapshot so the families lock is never held across the db lock below
    // (`register_web_font` takes them in the opposite order, scoped).
    let families = fonts().families.lock().unwrap().clone();

    // 1. Try exact match first
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        ..Default::default()
    };
    if fonts().db.lock().unwrap().query(&query).is_some() {
        if let Some(canonical) = families.iter().find(|&f| f.eq_ignore_ascii_case(family)) {
            return Some(canonical.clone());
        }
        return Some(family.to_string());
    }

    // 2. Try case-insensitive match on the families we have
    if let Some(matched) = families.iter().find(|&f| f.eq_ignore_ascii_case(family)) {
        return Some(matched.clone());
    }

    // 3. Match common prefixes / variations
    let family_lower = family.to_lowercase();
    let alias = match family_lower.as_str() {
        "arialn" => Some("Arial Narrow"),
        "gothic" => Some("Century Gothic"),
        "times" => Some("Times New Roman"),
        "cour" => Some("Courier New"),
        _ => None,
    };

    if let Some(alias_name) = alias {
        if let Some(matched) = families
            .iter()
            .find(|&f| f.eq_ignore_ascii_case(alias_name))
        {
            return Some(matched.clone());
        }
    }

    // 3b. Swiss 721 is Bitstream's Helvetica: without it, a free Helvetica
    //     clone keeps the text's widths. The condensed cut (`Swis721 Cn BT`,
    //     `swissc*.ttf`) only takes a narrow face, so lines still fit.
    if family_lower.starts_with("swis721") || family_lower.starts_with("swiss") {
        let condensed = family_lower.starts_with("swissc")
            || family_lower.split_whitespace().any(|word| word == "cn");
        let twins: &[&str] = if condensed {
            &[
                "TeX Gyre Heros Cn",
                "Nimbus Sans Narrow",
                "Arial Narrow",
                "Liberation Sans Narrow",
            ]
        } else {
            &[
                "TeX Gyre Heros",
                "Nimbus Sans",
                "Helvetica",
                "Arial",
                "Liberation Sans",
            ]
        };
        for twin in twins {
            if let Some(matched) = families.iter().find(|&f| f.eq_ignore_ascii_case(twin)) {
                return Some(matched.clone());
            }
        }
    }

    // 4. Web: the common office/CAD families the client cannot have installed
    //    stand in via the fetched Noto set (#141). Runs before the fuzzy prefix
    //    match so "Arial" can never drift onto an arbitrary Noto name.
    #[cfg(target_arch = "wasm32")]
    if let Some(target) = alias_resolution(&family_lower, |name| {
        families.iter().any(|f| f.eq_ignore_ascii_case(name))
    }) {
        if let Some(matched) = families.iter().find(|&f| f.eq_ignore_ascii_case(target)) {
            return Some(matched.clone());
        }
    }

    // 5. Try matching prefix/subset case-insensitively. Require at least 3
    //    chars so a 1–2 letter request can't grab an arbitrary family by the
    //    first iteration order. Iterating sorted keeps the pick deterministic.
    if family_lower.len() >= 3 {
        let mut candidates: Vec<&String> = families
            .iter()
            .filter(|&f| {
                let f_low = f.to_lowercase();
                f_low.starts_with(&family_lower) || family_lower.starts_with(&f_low)
            })
            .collect();
        candidates.sort();
        if let Some(matched) = candidates.first() {
            return Some((*matched).clone());
        }
    }

    None
}

/// Separator between a family and the face of it a [`styled_family`] name
/// asks for; a control character no family name contains.
pub const STYLE_SEP: char = '\u{2}';

/// A [`styled_family`] name split into the family and whether it asks for
/// the bold and the italic face. A plain family asks for neither.
pub fn split_style(name: &str) -> (&str, bool, bool) {
    match name.split_once(STYLE_SEP) {
        Some((family, style)) => (family, style.contains('b'), style.contains('i')),
        None => (name, false, false),
    }
}

/// The family name that draws `family`'s own bold and/or italic face — a
/// separate face, often a separate file (`GOTHICB.TTF`) — wherever a family
/// name goes (glyph outlines, shaping, PDF embedding). Only what the
/// installed faces really have is asked for; `None` when they have neither,
/// so the caller keeps the plain family.
pub fn styled_family(family: &str, bold: bool, italic: bool) -> Option<String> {
    let (base, was_bold, was_italic) = split_style(family);
    let (bold, italic) = (bold || was_bold, italic || was_italic);
    if !bold && !italic {
        return None;
    }
    let canonical = canonical_family_name(base)?;
    let db = fonts().db.lock().unwrap();
    let face = db.face(db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(&canonical)],
        weight: if bold {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        },
        style: if italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        },
        ..Default::default()
    })?)?;
    let bold = bold && face.weight.0 >= fontdb::Weight::SEMIBOLD.0;
    let italic = italic && face.style != fontdb::Style::Normal;
    (bold || italic).then(|| {
        let b = if bold { "b" } else { "" };
        let i = if italic { "i" } else { "" };
        format!("{canonical}{STYLE_SEP}{b}{i}")
    })
}

/// Resolve a family name to a concrete face id (regular weight/style, or
/// the face a [`styled_family`] name asks for).
fn face_id(family: &str) -> Option<fontdb::ID> {
    face_id_weighted(family, fontdb::Weight::NORMAL)
}

/// Resolve a family name to a concrete face id at the requested weight
/// (e.g. bold runs). Falls back through fontdb's fuzzy matching, so a
/// family without a bold face still resolves — to its regular face.
fn face_id_weighted(family: &str, weight: fontdb::Weight) -> Option<fontdb::ID> {
    let (family, bold, italic) = split_style(family);
    let canonical = canonical_family_name(family)?;
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(&canonical)],
        weight: if bold { fontdb::Weight::BOLD } else { weight },
        style: if italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        },
        ..Default::default()
    };
    fonts().db.lock().unwrap().query(&query)
}

/// The file name (lower case) of the installed face of exactly `family` —
/// no stand-in family — in the bold and italic asked for, or `None`.
pub fn face_file(family: &str, bold: bool, italic: bool) -> Option<String> {
    let db = fonts().db.lock().unwrap();
    let id = db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        weight: if bold {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        },
        style: if italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        },
        ..Default::default()
    })?;
    match &db.face(id)?.source {
        fontdb::Source::File(path) => {
            Some(path.file_name()?.to_string_lossy().to_ascii_lowercase())
        }
        _ => None,
    }
}

/// Borrow the raw face bytes for `family` and run `f` over them. The byte slice
/// is only valid inside the closure, so callers extract everything they need
/// (e.g. flattened glyph outlines) before returning. `index` is the face index
/// within a TrueType collection. Returns `None` if the family is unknown.
pub fn with_face_data<T>(family: &str, f: impl FnOnce(&[u8], u32) -> T) -> Option<T> {
    let id = face_id(family)?;
    fonts().db.lock().unwrap().with_face_data(id, f)
}

/// Borrow the raw face bytes for `family` at `weight` (see
/// [`face_id_weighted`]). The PDF exporter uses this to embed the bold face
/// for bold runs instead of faux-stroking the regular one.
pub fn with_face_data_weighted<T>(
    family: &str,
    weight: fontdb::Weight,
    f: impl FnOnce(&[u8], u32) -> T,
) -> Option<T> {
    let id = face_id_weighted(family, weight)?;
    fonts().db.lock().unwrap().with_face_data(id, f)
}

/// Original bytes and file name of an explicitly resolved TrueType/OpenType
/// face for ETRANSMIT. A file-name reference is preferred; a family reference
/// must match exactly, never one of the renderer's metric-compatible aliases.
/// On the web only host-registered file bytes qualify, so a fetched Noto
/// substitute is never mislabeled as the drawing's original proprietary font.
pub(crate) fn transmittal_face(
    reference: &str,
    bold: bool,
    italic: bool,
) -> Option<(String, Vec<u8>)> {
    let explicit = file_face(reference);
    #[cfg(target_arch = "wasm32")]
    if explicit.is_none() {
        return None;
    }
    let family = explicit.as_ref().map(|face| face.family.clone());
    #[cfg(not(target_arch = "wasm32"))]
    let family = family.or_else(|| {
        canonical_family_name(reference).filter(|family| family.eq_ignore_ascii_case(reference))
    });
    let id = if let Some(face) = explicit {
        // A file reference identifies these exact bytes, even when another
        // installed face has the same family and weight.
        face.id
    } else {
        let family = family?;
        let (base, was_bold, was_italic) = split_style(&family);
        fonts().db.lock().unwrap().query(&fontdb::Query {
            families: &[fontdb::Family::Name(base)],
            weight: if bold || was_bold {
                fontdb::Weight::BOLD
            } else {
                fontdb::Weight::NORMAL
            },
            style: if italic || was_italic {
                fontdb::Style::Italic
            } else {
                fontdb::Style::Normal
            },
            ..Default::default()
        })?
    };
    let db = fonts().db.lock().unwrap();
    let face = db.face(id)?;
    let file_name = match &face.source {
        fontdb::Source::File(path) => path.file_name()?.to_string_lossy().into_owned(),
        _ => reference
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| name.contains('.'))?
            .to_string(),
    };
    let bytes = db.with_face_data(id, |data, _| data.to_vec())?;
    Some((file_name, bytes))
}

/// The family of the installed face whose file is named `name` — a bare file
/// name or any path to one, matched case-insensitively.
fn file_face(name: &str) -> Option<FileFace> {
    let file = name.rsplit(['/', '\\']).next()?.trim();
    if file.is_empty() {
        return None;
    }
    fonts()
        .by_file
        .lock()
        .unwrap()
        .get(&file.to_ascii_lowercase())
        .cloned()
}

pub(crate) fn family_for_file(name: &str) -> Option<String> {
    file_face(name).map(|face| face.family)
}

/// The installed family a drawing's font reference names, or `None` when
/// nothing installed answers to it.
///
/// A DWG text style stores the TrueType font *file* (`GOST2304_TypeA_italic.ttf`,
/// sometimes with a directory), which is what the style resolves by; the family name
/// lives inside the file and is usually not the same string. Inline `\f` codes
/// carry either form, so both are tried.
pub fn family_for_reference(reference: &str) -> Option<String> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    // The file name is exact; the family lookup ends in a prefix/subset match
    // that a longer file name loses to (`ISOCPEUI.TTF` would take `ISOCP`), so
    // an installed face file answers first.
    family_for_file(reference).or_else(|| canonical_family_name(reference))
}

/// Resolve a file-valued font reference against its owning drawing before the
/// global basename/family fallback. This is what lets Preserve packages carry
/// same-named TrueType files in different source-relative folders.
pub fn family_for_reference_at(reference: &str, base: Option<&std::path::Path>) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = crate::io::font_repo::font_file_path(reference, base) {
        let path = std::path::PathBuf::from(path);
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if let Some(face) = fonts().by_path.lock().unwrap().get(&path).cloned() {
            return Some(face.family);
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = base;
    family_for_reference(reference)
}

/// Whether `family` matches an installed system font (case-insensitive via
/// fontdb's own matching).
pub fn has_family(family: &str) -> bool {
    face_id(family).is_some()
}

/// Web: add a fetched font's bytes (a Noto subset, see `web_font`) to the
/// database so family-name resolution starts finding it. Fetches land after
/// the first text layout pass, so the family list is rebuilt and the
/// canonical-name memo cleared — negative resolutions from while the font was
/// in flight must not stick. Callers invalidate the glyph caches separately
/// (`ttf_glyph::clear_fallback_cache`).
///
/// Faces may repeat across subsets (every file carries the Latin face, and the
/// CJK collections repeat it per language); `family_names` de-duplicates the
/// list, and fontdb picks one of the identical Latin faces per query.
#[cfg(target_arch = "wasm32")]
pub fn register_web_font(bytes: &[u8]) {
    let sys = fonts();
    let names = {
        let mut db = sys.db.lock().unwrap();
        db.load_font_data(bytes.to_vec());
        family_names(&db)
        // db lock released before the families lock is taken — the resolution
        // path takes them in the opposite order.
    };
    *sys.families.lock().unwrap() = names;
    canonical_cache().lock().unwrap().clear();
}

/// Add a font file a host supplies at run time (the `font_register` control
/// op): its faces join the database and `file_name` resolves to the family of
/// the first face, so a text style naming that file finds it. Returns that
/// family and the number of faces the file added, or `None` when the bytes
/// hold no face fontdb can read. Like [`register_web_font`], the family list
/// is rebuilt and the canonical-name memo cleared; callers invalidate the glyph
/// caches and re-layout text.
pub fn register_font_file(file_name: &str, bytes: &[u8]) -> Option<(String, usize)> {
    let file = file_name.rsplit(['/', '\\']).next()?.trim();
    if file.is_empty() {
        return None;
    }
    let sys = fonts();
    let (id, family, faces, names) = {
        let mut db = sys.db.lock().unwrap();
        let before: std::collections::HashSet<fontdb::ID> = db.faces().map(|f| f.id).collect();
        db.load_font_data(bytes.to_vec());
        let added: Vec<(fontdb::ID, String)> = db
            .faces()
            .filter(|face| !before.contains(&face.id))
            .filter_map(|face| {
                face.families
                    .first()
                    .map(|(name, _)| (face.id, name.clone()))
            })
            .collect();
        let (id, family) = added.first()?.clone();
        (id, family, added.len(), family_names(&db))
    };
    sys.by_file.lock().unwrap().insert(
        file.to_ascii_lowercase(),
        FileFace {
            id,
            family: family.clone(),
        },
    );
    *sys.families.lock().unwrap() = names;
    canonical_cache().lock().unwrap().clear();
    Some((family, faces))
}

/// Register TrueType/OpenType files referenced by a drawing, beside it, and in
/// ancestor `Fonts` folders. Source-relative Preserve paths are included, not
/// just the flat and Structured package defaults.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn load_drawing_fonts(base: Option<&std::path::Path>, doc: &document::Document) {
    static LOADED: OnceLock<Mutex<std::collections::HashSet<std::path::PathBuf>>> = OnceLock::new();

    let mut dirs: Vec<_> = base
        .into_iter()
        .map(std::path::Path::to_path_buf)
        .chain(crate::io::font_repo::packaged_font_dirs(base))
        .collect();
    for style in doc.text_styles.iter() {
        for reference in [&style.font_file, &style.true_type_font] {
            let is_face_file = std::path::Path::new(reference)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    ["ttf", "otf", "ttc"]
                        .iter()
                        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
                });
            if !is_face_file {
                continue;
            }
            if let Some(parent) = crate::io::resolve_image_file(reference, base)
                .map(std::path::PathBuf::from)
                .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
            {
                dirs.push(parent);
            }
        }
    }
    let dirs: Vec<_> = dirs
        .into_iter()
        .filter(|dir| dir.is_dir())
        .map(|dir| std::fs::canonicalize(&dir).unwrap_or(dir))
        .collect();
    if dirs.is_empty() {
        return;
    }
    let loaded = LOADED.get_or_init(Default::default);
    let mut loaded = loaded.lock().unwrap();
    let dirs: Vec<_> = dirs
        .into_iter()
        .filter(|dir| loaded.insert(dir.clone()))
        .collect();
    if dirs.is_empty() {
        return;
    }

    let sys = fonts();
    let (added, names) = {
        let mut db = sys.db.lock().unwrap();
        let before: std::collections::HashSet<fontdb::ID> =
            db.faces().map(|face| face.id).collect();
        for dir in dirs {
            db.load_fonts_dir(dir);
        }
        let added: Vec<(String, std::path::PathBuf, FileFace)> = db
            .faces()
            .filter(|face| !before.contains(&face.id))
            .filter_map(|face| {
                let fontdb::Source::File(path) = &face.source else {
                    return None;
                };
                let file = path.file_name()?.to_string_lossy().to_ascii_lowercase();
                let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                let (family, _) = face.families.first()?;
                Some((
                    file,
                    resolved,
                    FileFace {
                        id: face.id,
                        family: face_family(face, family),
                    },
                ))
            })
            .collect();
        (added, family_names(&db))
    };
    if added.is_empty() {
        return;
    }
    {
        let mut by_file = sys.by_file.lock().unwrap();
        for (file, path, face) in added {
            // A drawing's own font file wins over an installed file with the
            // same name, just as drawing-relative SHX lookup wins. The exact
            // resolved path remains available for owner-relative resolution.
            by_file.insert(file, face.clone());
            sys.by_path.lock().unwrap().insert(path, face);
        }
        *sys.families.lock().unwrap() = names;
    }
    canonical_cache().lock().unwrap().clear();
    crate::scene::text::ttf_glyph::clear_fallback_cache();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host-registered file resolves by its file name — any case, bare or
    /// with a path — to the family inside it; bytes without a face are refused
    /// and leave nothing behind.
    #[test]
    fn registered_font_file_resolves_by_file_name() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/web/fonts/greek.ttf"))
            .expect("bundled web font");
        let (family, faces) =
            register_font_file("C:\\Fonts\\HostRegistered-Test.ttf", &bytes).expect("a face");
        assert!(faces >= 1);
        assert!(!family.is_empty());
        assert_eq!(
            family_for_reference("hostregistered-test.TTF").as_deref(),
            Some(family.as_str())
        );
        assert_eq!(
            family_for_reference("fonts/HostRegistered-Test.ttf").as_deref(),
            Some(family.as_str())
        );
        assert!(has_family(&family));
        let (file_name, packaged) =
            transmittal_face("fonts/HostRegistered-Test.ttf", false, false).unwrap();
        assert_eq!(file_name, "HostRegistered-Test.ttf");
        assert_eq!(packaged, bytes);

        assert_eq!(register_font_file("Broken-Test.ttf", b"not a font"), None);
        assert_eq!(family_for_reference("Broken-Test.ttf"), None);
        assert_eq!(register_font_file("", &bytes), None);
    }

    /// A DWG stores a style's TrueType font as a file name, so every installed
    /// face has to be reachable by the name of its file — case-insensitively,
    /// and whether it comes as a bare name or as a path.
    #[test]
    fn face_files_resolve_to_their_family() {
        let found = fonts()
            .db
            .lock()
            .unwrap()
            .faces()
            .find_map(|face| match &face.source {
                fontdb::Source::File(path) => {
                    let file = path.file_name()?.to_string_lossy().into_owned();
                    let family = face.families.first().map(|(name, _)| name.clone())?;
                    Some((file, family))
                }
                _ => None,
            });
        let Some((file, family)) = found else {
            eprintln!("no installed face files on this host; skipping");
            return;
        };
        // A bold / italic file names that face of the family.
        let family_of = |name: &str| family_for_file(name).map(|f| split_style(&f).0.to_string());
        let family_of_reference =
            |name: &str| family_for_reference(name).map(|f| split_style(&f).0.to_string());

        assert_eq!(family_of(&file).as_deref(), Some(family.as_str()));
        assert_eq!(
            family_of(&file.to_ascii_uppercase()).as_deref(),
            Some(family.as_str())
        );
        assert_eq!(
            family_of_reference(&format!("/somewhere/else/{file}")).as_deref(),
            Some(family.as_str())
        );
        assert_eq!(family_for_file("no-such-face-9f3.ttf"), None);
        assert_eq!(family_for_reference("   "), None);
    }

    #[test]
    fn sans_aliases_map_to_noto_sans() {
        for name in [
            "Arial",
            "Helvetica",
            "Verdana",
            "Tahoma",
            "Calibri",
            "Segoe UI",
            "Liberation Sans",
            "DejaVu Sans",
        ] {
            assert_eq!(
                alias_targets(&name.to_lowercase()),
                Some(&["Noto Sans"][..]),
                "{name} must alias to Noto Sans"
            );
        }
    }

    #[test]
    fn serif_aliases_prefer_serif_with_sans_fallback() {
        for name in [
            "Cambria",
            "Times New Roman",
            "Georgia",
            "Liberation Serif",
            "DejaVu Serif",
        ] {
            assert_eq!(
                alias_targets(&name.to_lowercase()),
                Some(&["Noto Serif", "Noto Sans"][..]),
                "{name} must alias to Noto Serif first, Noto Sans second"
            );
        }
    }

    #[test]
    fn alias_resolution_is_case_insensitive_and_needs_a_target() {
        // Only the sans stand-in available (today's web fetch set): a serif
        // alias degrades to Noto Sans rather than resolving to nothing.
        assert_eq!(
            alias_resolution("arial", |f| f == "Noto Sans"),
            Some("Noto Sans")
        );
        assert_eq!(
            alias_resolution("ARIAL", |f| f == "Noto Sans"),
            Some("Noto Sans")
        );
        assert_eq!(
            alias_resolution("cambria", |f| f == "Noto Sans"),
            Some("Noto Sans")
        );
        // Serif preferred when a serif subset is present.
        assert_eq!(
            alias_resolution("cambria", |f| f == "Noto Serif"),
            Some("Noto Serif")
        );
        // Nothing registered yet — fetch still in flight.
        assert_eq!(alias_resolution("arial", |_| false), None);
        // Unknown / stroke-font names are never aliased.
        assert_eq!(alias_resolution("txt", |_| true), None);
        assert_eq!(alias_resolution("isocp", |_| true), None);
        assert_eq!(alias_resolution("monospace", |_| true), None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn transmittal_font_dirs_register_exact_truetype_files() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/web/fonts/greek.ttf"))
            .expect("bundled web font");
        for (tag, nested, structured) in [
            ("root", false, true),
            ("xref", true, true),
            ("flat", false, false),
        ] {
            let dir = std::env::temp_dir().join(format!(
                "ocs-sysfont-transmittal-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let fonts = if structured {
                dir.join("Fonts")
            } else {
                dir.clone()
            };
            let base = if nested {
                dir.join("xref/discipline")
            } else {
                dir.clone()
            };
            std::fs::create_dir_all(&fonts).unwrap();
            std::fs::create_dir_all(&base).unwrap();
            let file = format!("Packaged-{tag}-{}.ttf", std::process::id());
            std::fs::write(fonts.join(&file), &bytes).unwrap();

            load_drawing_fonts(Some(&base), &document::Document::new());

            let (_, packaged) = transmittal_face(&file, false, false)
                .unwrap_or_else(|| panic!("{file} was not registered from {}", fonts.display()));
            assert_eq!(packaged, bytes);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn preserved_relative_fonts_resolve_by_owning_drawing() {
        let dir = std::env::temp_dir().join(format!(
            "ocs-sysfont-preserved-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for (folder, source) in [("first", "greek.ttf"), ("second", "latin.ttf")] {
            let base = dir.join(folder);
            std::fs::create_dir_all(base.join("assets")).unwrap();
            std::fs::write(
                base.join("assets/shared.ttf"),
                std::fs::read(
                    concat!(env!("CARGO_MANIFEST_DIR"), "/web/fonts/").to_owned() + source,
                )
                .unwrap(),
            )
            .unwrap();
            let mut doc = document::Document::new();
            let mut style = document::tables::TextStyle::new("Preserved");
            style.font_file = "assets/shared.ttf".into();
            doc.text_styles.add(style).unwrap();
            load_drawing_fonts(Some(&base), &doc);
        }

        let mut ids = Vec::new();
        for folder in ["first", "second"] {
            let base = dir.join(folder);
            let path = std::fs::canonicalize(base.join("assets/shared.ttf")).unwrap();
            let expected = fonts().by_path.lock().unwrap().get(&path).cloned().unwrap();
            assert_eq!(
                family_for_reference_at("assets/shared.ttf", Some(&base)),
                Some(expected.family)
            );
            ids.push(expected.id);
        }
        assert_ne!(ids[0], ids[1]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn picker_aliases_only_once_noto_arrives() {
        // Before any fetch lands the picker must not offer unresolvable names.
        let mut empty: Vec<String> = Vec::new();
        merge_web_aliases(&mut empty);
        assert!(empty.is_empty());

        // With the Noto set present the aliases join, sorted and de-duplicated.
        let mut with_noto = vec!["Noto Sans CJK SC".to_string(), "Noto Sans".to_string()];
        merge_web_aliases(&mut with_noto);
        assert_eq!(
            with_noto,
            vec![
                "Arial".to_string(),
                "Book Antiqua".to_string(),
                "Calibri".to_string(),
                "Cambria".to_string(),
                "Constantia".to_string(),
                "DejaVu Sans".to_string(),
                "DejaVu Serif".to_string(),
                "Garamond".to_string(),
                "Georgia".to_string(),
                "Helvetica".to_string(),
                "Liberation Sans".to_string(),
                "Liberation Serif".to_string(),
                "Noto Sans".to_string(),
                "Noto Sans CJK SC".to_string(),
                "Palatino Linotype".to_string(),
                "Segoe UI".to_string(),
                "Tahoma".to_string(),
                "Times New Roman".to_string(),
                "Verdana".to_string(),
            ]
        );

        // A registry that already carries an alias name (case differs) dedupes.
        let mut dup = vec!["noto sans".to_string()];
        merge_web_aliases(&mut dup);
        assert_eq!(
            dup.iter()
                .filter(|f| f.eq_ignore_ascii_case("Noto Sans"))
                .count(),
            1
        );
    }
}
