// System TrueType/OpenType font discovery.
//
// Wraps a `fontdb` database loaded with the user's installed system fonts so
// the rest of the app can (a) list available font families for the text-style
// picker and (b) borrow a face's raw bytes to extract glyph outlines (see the
// TTF glyph engine). LFF stroke fonts stay separate — this is purely the
// TrueType side of the renderer.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

struct SysFonts {
    db: fontdb::Database,
    /// Sorted, de-duplicated family names for the picker.
    families: Vec<String>,
}

static FONTS: OnceLock<SysFonts> = OnceLock::new();

fn fonts() -> &'static SysFonts {
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let mut families: Vec<String> = db
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .collect();
        families.sort_by_key(|n| n.to_lowercase());
        families.dedup();

        SysFonts { db, families }
    })
}

/// All installed system font families, sorted case-insensitively, de-duped.
pub fn families() -> &'static [String] {
    &fonts().families
}

/// Resolve a requested family name to the canonical installed system family name (with exact case).
///
/// Memoised process-wide: `resolve_font` calls this once per word on the MTEXT
/// measure hot path for inline-`\f` TTF runs, and `Face::resolve` re-runs it
/// immediately after — the underlying fontdb query plus linear family scans are
/// not free. The cache keys on the raw request string; results are stable for
/// the process lifetime (the font DB is loaded once via `OnceLock`).
pub fn canonical_family_name(family: &str) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache.lock().unwrap().get(family) {
        return hit.clone();
    }
    let resolved = canonical_family_name_uncached(family);
    cache
        .lock()
        .unwrap()
        .insert(family.to_string(), resolved.clone());
    resolved
}

fn canonical_family_name_uncached(family: &str) -> Option<String> {
    let db = &fonts().db;

    // 1. Try exact match first
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        ..Default::default()
    };
    if db.query(&query).is_some() {
        if let Some(canonical) = fonts()
            .families
            .iter()
            .find(|&f| f.eq_ignore_ascii_case(family))
        {
            return Some(canonical.clone());
        }
        return Some(family.to_string());
    }

    // 2. Try case-insensitive match on the families we have
    if let Some(matched) = fonts()
        .families
        .iter()
        .find(|&f| f.eq_ignore_ascii_case(family))
    {
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
        if let Some(matched) = fonts()
            .families
            .iter()
            .find(|&f| f.eq_ignore_ascii_case(alias_name))
        {
            return Some(matched.clone());
        }
    }

    // 4. Try matching prefix/subset case-insensitively. Require at least 3
    //    chars so a 1–2 letter request can't grab an arbitrary family by the
    //    first iteration order. Iterating sorted keeps the pick deterministic.
    if family_lower.len() >= 3 {
        let mut candidates: Vec<&String> = fonts()
            .families
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

/// Resolve a family name to a concrete face id (regular weight/style).
fn face_id(family: &str) -> Option<fontdb::ID> {
    let db = &fonts().db;
    let canonical = canonical_family_name(family)?;
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(&canonical)],
        ..Default::default()
    };
    db.query(&query)
}

/// Borrow the raw face bytes for `family` and run `f` over them. The byte slice
/// is only valid inside the closure, so callers extract everything they need
/// (e.g. flattened glyph outlines) before returning. `index` is the face index
/// within a TrueType collection. Returns `None` if the family is unknown.
pub fn with_face_data<T>(family: &str, f: impl FnOnce(&[u8], u32) -> T) -> Option<T> {
    let id = face_id(family)?;
    fonts().db.with_face_data(id, f)
}

/// Whether `family` matches an installed system font (case-insensitive via
/// fontdb's own matching).
pub fn has_family(family: &str) -> bool {
    face_id(family).is_some()
}
