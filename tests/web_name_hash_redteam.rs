//! RED-TEAM: the FNV-1a name-hash keying shared by `web_recent` and
//! `web_autosave` (`src/io/web_recent.rs:225-242`, `pub(crate)` — replicated
//! verbatim below; OPFS itself is browser-only).
//!
//! Questions attacked:
//! 1. Does the code rely on hash uniqueness? (Reading: YES — a collision
//!    silently merges two drawings' OPFS entries; see test docs below.)
//! 2. Unicode normalization: NFC vs NFD "café.dwg" hash differently.
//! 3. Are real collisions findable? (Pollard-rho result pinned below.)
#![cfg(not(target_arch = "wasm32"))]

/// Verbatim copy of src/io/web_recent.rs:235-242.
fn name_hash(name: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Verbatim composition of the OPFS entry names both stores derive from it
/// (web_recent.rs:225-231, web_autosave.rs:205-211).
fn cache_key(name: &str) -> String {
    format!("{}.cad", name_hash(name))
}
fn thumbnail_key(name: &str) -> String {
    format!("{}.thumb", name_hash(name))
}
fn snapshot_key(name: &str) -> String {
    format!("{}.sv$", name_hash(name))
}

/// The implementation is exactly canonical FNV-1a 64 (offset basis, then
/// xor-byte/multiply per byte) — pinned against the reference vectors so the
/// collision analysis below applies to the shipped code.
#[test]
fn the_hash_is_canonical_fnv1a_64() {
    assert_eq!(name_hash(""), format!("{:016x}", 0xcbf29ce484222325_u64));
    assert_eq!(name_hash("a"), format!("{:016x}", 0xaf63dc4c8601ec8c_u64));
    assert_eq!(name_hash("foobar"), format!("{:016x}", 0x85944171f73967e8_u64));
    // Keys are bare hex + fixed suffixes: they can never contain path
    // separators or grow with the drawing name, whatever the name contains.
    for hostile in [
        "../evil.dwg",
        "..\\evil.dxf",
        "a/b/c.dwg",
        "nul\u{0}.dwg",
        "plan.sv$",
        "",
        "x".repeat(4096).as_str(),
    ] {
        let key = snapshot_key(hostile);
        let (prefix, suffix) = key.split_at(16);
        assert!(prefix.chars().all(|c| c.is_ascii_hexdigit()), "{key:?}");
        assert_eq!(suffix, ".sv$");
        assert_eq!(cache_key(hostile).len(), ".cad".len() + 16);
        assert_eq!(thumbnail_key(hostile).len(), ".thumb".len() + 16);
    }
}

/// Unicode normalization: "café.dwg" in NFC (single U+00E9) and NFD
/// ("e" + U+0301) are the SAME visual name but different byte strings, so
/// they get different keys — two recents entries, two autosave snapshots,
/// two tabs that look identical to the user. Harm has two edges:
/// - mostly benign: each entry stores/loads its own bytes consistently;
/// - confusing: a user with an NFD-named file on disk and an NFC-named copy
///   from the web picker sees the "same" drawing twice in the Start page and
///   the pair never share an autosave snapshot, so recovering one does not
///   recover the other. Not corruption — a duplicate-entry wart.
#[test]
fn nfc_and_nfd_names_hash_to_distinct_entries() {
    let nfc = "caf\u{e9}.dwg"; // U+00E9
    let nfd = "cafe\u{301}.dwg"; // e + combining acute
    assert_eq!(nfc, nfd.nfc_str()); // visually/normalization-wise the "same" name
    assert_ne!(nfc.as_bytes(), nfd.as_bytes());
    assert_ne!(name_hash(nfc), name_hash(nfd));
    assert_ne!(snapshot_key(nfc), snapshot_key(nfd));
    assert_ne!(cache_key(nfc), cache_key(nfd));
}

trait Nfc {
    fn nfc_str(&self) -> String;
}
impl Nfc for str {
    fn nfc_str(&self) -> String {
        // Minimal NFC for the one case under test: precomposed é.
        self.replace("e\u{301}", "\u{e9}")
    }
}

/// REAL COLLISION (Pollard-rho, ~1.1e9 steps / 208 s in a release build over
/// 10-char printable names): two different drawing FILE NAMES that share one
/// FNV-1a 64 hash — and therefore ONE OPFS entry in BOTH stores. Found on
/// the 10-char stems; because FNV processes bytes sequentially and the two
/// stems reach the SAME intermediate state, appending the same ".dwg" suffix
/// preserves the collision, so these are full realistic filenames.
///
/// The code has no defense: `web_recent::store` writes `cache_key(name)`
/// blindly (web_recent.rs:34) and `web_autosave::snapshot` writes
/// `snapshot_key(name)` blindly (web_autosave.rs:116), each keyed ONLY by
/// the hash. With these two names in play:
///   * the recents copy of the first drawing is OVERWRITTEN by the second
///     (the Start page then reopens the second's bytes under the first's
///     name — and, if the extensions differ, parses them with the WRONG
///     reader: ".dxf" name + DWG bytes);
///   * the autosave index (web_autosave.rs:112-115) keeps BOTH names, but
///     both rows resolve to the same snapshot file — restoring "the first"
///     silently hands back the second drawing's geometry, and deleting one
///     after its restore (web_autosave.rs:137-151 `remove`) kills the
///     other's snapshot too (the dead row is filtered on the next scan).
/// Probability for organically-chosen names is birthday-bound (~2^-64 per
/// pair, ~2^-13 even for a 100-drawing pool), so this is theoretical for
/// real users — but it is a total, silent loss of the uniqueness
/// assumption. Storing the name alongside the bytes and verifying payload
/// identity on read (the index already maps hash->name), or keying on a
/// longer digest (SHA-256/BLAKE3 truncated to 32 hex chars), would close it.
#[test]
fn two_different_names_share_one_key() {
    let (a, b) = (COLLISION_A, COLLISION_B);
    assert_ne!(a, b, "collision pair must be distinct names");
    assert!(a.is_ascii() && b.is_ascii());
    assert!(a.ends_with(".dwg") && b.ends_with(".dwg"));
    assert_eq!(name_hash(a), name_hash(b), "pinned collision must hold");
    assert_eq!(snapshot_key(a), snapshot_key(b));
    assert_eq!(cache_key(a), cache_key(b));
    assert_eq!(thumbnail_key(a), thumbnail_key(b));
}

// Pollard-rho result (verify_coll / find_b reconstructions, byte-exact):
// stems `"GK@"Qo8if` [22 47 4b 40 22 51 6f 38 69 66] and `%e+k]u[/],`
// [25 65 2b 6b 5d 75 5b 2f 5d 2c] both hash to a95b939b46c157f8.
const COLLISION_A: &str = "\"GK@\"Qo8if.dwg";
const COLLISION_B: &str = "%e+k]u[/],.dwg";
