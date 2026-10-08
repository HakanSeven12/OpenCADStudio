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
    /// Installed face file name (lower-case) → the family that face holds.
    /// A drawing references a TrueType style by *file* name, so this is what
    /// makes such a style resolve — see [`family_for_reference`].
    by_file: HashMap<String, String>,
}

static FONTS: OnceLock<SysFonts> = OnceLock::new();

fn fonts() -> &'static SysFonts {
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let mut families: Vec<String> = Vec::new();
        let mut by_file: HashMap<String, String> = HashMap::new();
        for face in db.faces() {
            let Some((family, _)) = face.families.first() else {
                continue;
            };
            families.push(family.clone());
            // Remember every face file: a DWG style names the file, and the
            // family inside it is usually a different string.
            if let fontdb::Source::File(path) = &face.source {
                if let Some(name) = path.file_name() {
                    by_file
                        .entry(name.to_string_lossy().to_ascii_lowercase())
                        .or_insert_with(|| family.clone());
                }
            }
        }
        families.sort_by_key(|n| n.to_lowercase());
        families.dedup();

        SysFonts {
            db,
            families,
            by_file,
        }
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
        if let Some(canonical) = fonts().families.iter().find(|&f| f.eq_ignore_ascii_case(family)) {
            return Some(canonical.clone());
        }
        return Some(family.to_string());
    }
    
    // 2. Try case-insensitive match on the families we have
    if let Some(matched) = fonts().families.iter().find(|&f| f.eq_ignore_ascii_case(family)) {
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
        if let Some(matched) = fonts().families.iter().find(|&f| f.eq_ignore_ascii_case(alias_name)) {
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
    face_id_weighted(family, fontdb::Weight::NORMAL)
}

/// Resolve a family name to a concrete face id at the requested weight
/// (e.g. bold runs). Falls back through fontdb's fuzzy matching, so a
/// family without a bold face still resolves — to its regular face.
fn face_id_weighted(family: &str, weight: fontdb::Weight) -> Option<fontdb::ID> {
    let db = &fonts().db;
    let canonical = canonical_family_name(family)?;
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(&canonical)],
        weight,
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

/// Borrow the raw face bytes for `family` at `weight` (see
/// [`face_id_weighted`]). The PDF exporter uses this to embed the bold face
/// for bold runs instead of faux-stroking the regular one.
pub fn with_face_data_weighted<T>(
    family: &str,
    weight: fontdb::Weight,
    f: impl FnOnce(&[u8], u32) -> T,
) -> Option<T> {
    let id = face_id_weighted(family, weight)?;
    fonts().db.with_face_data(id, f)
}

/// The family of the installed face whose file is named `name` — a bare file
/// name or any path to one, matched case-insensitively.
fn family_for_file(name: &str) -> Option<String> {
    let file = name.rsplit(['/', '\\']).next()?.trim();
    if file.is_empty() {
        return None;
    }
    fonts().by_file.get(&file.to_ascii_lowercase()).cloned()
}

/// The installed family a drawing's font reference names, or `None` when
/// nothing installed answers to it.
///
/// A DWG text style stores the TrueType font *file* (`GOST2304_TypeA_italic.ttf`,
/// sometimes with a directory), which is what AutoCAD resolves; the family name
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

/// Whether `family` matches an installed system font (case-insensitive via
/// fontdb's own matching).
pub fn has_family(family: &str) -> bool {
    face_id(family).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DWG stores a style's TrueType font as a file name, so every installed
    /// face has to be reachable by the name of its file — case-insensitively,
    /// and whether it comes as a bare name or as a path.
    #[test]
    fn face_files_resolve_to_their_family() {
        let found = fonts().db.faces().find_map(|face| match &face.source {
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

        assert_eq!(family_for_file(&file).as_deref(), Some(family.as_str()));
        assert_eq!(
            family_for_file(&file.to_ascii_uppercase()).as_deref(),
            Some(family.as_str())
        );
        assert_eq!(
            family_for_reference(&format!("/somewhere/else/{file}")).as_deref(),
            Some(family.as_str())
        );
        assert_eq!(family_for_file("no-such-face-9f3.ttf"), None);
        assert_eq!(family_for_reference("   "), None);
    }
}

