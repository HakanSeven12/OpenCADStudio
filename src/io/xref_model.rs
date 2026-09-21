// XREF path model — lexical identity for external references (Task 2).

/// Shared lexical walk for [`normalize_lexical`] (identity: case-folded on
/// Windows) and [`normalize_display`] (display: case-preserving). One
/// implementation so the two can never drift apart.
///
/// `fold_case` lowercases the drive prefix and the joined remainder.
/// UNC shares (`//server/share/…`) additionally clamp `..` at the share root
/// (`floor == 2`): climbing above the share is deterministic garbage
/// otherwise, and a share root behaves like a filesystem root for this
/// purpose. Drive-absolute paths clamp at `/`; relative paths retain
/// surplus `..` (it still means "up").
fn normalize_core(raw: &str, fold_case: bool) -> String {
    // 1. Separator unification. Pure string op — no I/O, so missing files
    // (the common NotFound case) normalize exactly like present ones.
    let slashed = raw.replace('\\', "/");

    // 2. Prefix split: drive (`C:`), UNC (`//`), or none.
    let bytes = slashed.as_bytes();
    let (prefix, mut rest, unc) = if bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
    {
        let drive = &slashed[..2];
        (
            if fold_case {
                drive.to_ascii_lowercase()
            } else {
                drive.to_string()
            },
            slashed[2..].to_string(),
            false,
        )
    } else if let Some(stripped) = slashed.strip_prefix("//") {
        ("//".to_string(), stripped.to_string(), true)
    } else {
        (String::new(), slashed, false)
    };
    if unc {
        // Collapse `///share` → `//share` so the rejoin can't triple-slash.
        rest = rest.trim_start_matches('/').to_string();
    }
    let absolute = rest.starts_with('/');
    // Components below this stack depth survive `..`: 2 for UNC
    // (server + share), 0 otherwise (absolute roots clamp via `absolute`).
    let floor = if unc { 2 } else { 0 };
    let climb = !(absolute || unc);

    // 3. Lexical component cleanup: drop empties/`.`, resolve `..` by
    // popping subject to the floor above.
    let mut stack: Vec<&str> = Vec::new();
    for comp in rest.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        } else if comp == ".." {
            if stack.len() > floor {
                stack.pop();
            } else if climb {
                stack.push("..");
            }
        } else {
            stack.push(comp);
        }
    }

    // 4. Case fold: Windows filesystems are case-insensitive, so identity
    // must be too. Native non-Windows paths remain case-sensitive.
    let mut joined = stack.join("/");
    // A drive/UNC path keeps Windows identity semantics even when the host
    // application runs elsewhere and is inspecting a drawing created there.
    if fold_case && (cfg!(windows) || !prefix.is_empty()) {
        joined = joined.to_lowercase();
    }

    // 5. Rejoin as `{prefix}{joined}`, preserving rootedness.
    if absolute {
        joined = format!("/{joined}");
    }
    format!("{prefix}{joined}")
}

/// Lexical path normalization for reference identity. Never touches the
/// filesystem (canonicalize fails on missing files; NotFound is common).
/// Backslash→slash, `.`/`..` component cleanup, case-fold on Windows.
pub fn normalize_lexical(raw: &str) -> String {
    // `fold_case` is passed as true unconditionally: `normalize_core` still
    // gates the actual fold on `cfg!(windows)`, so non-Windows builds keep
    // case-sensitive identity (folding there would conflate distinct files).
    // `root_key` lowercases its own input separately for root comparison, so
    // identity regimes stay aligned across targets without touching this.
    normalize_core(raw, true)
}

/// How an XREF path is stored relative to its host drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pathtype {
    Full,
    Relative,
    None,
}

/// Why a [`Pathtype::Relative`] conversion cannot be expressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathtypeError {
    AcrossDrives,
    UnsavedHost,
}

/// Case-preserving sibling of [`normalize_lexical`] for stored/display paths.
/// Same walk, no case folding.
fn normalize_display(raw: &str) -> String {
    normalize_core(raw, false)
}

fn root_key(normalized: &str) -> String {
    let lower = normalized.to_ascii_lowercase();
    let b = lower.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return lower[..2].to_string();
    }
    if lower.starts_with("//") {
        let mut parts = lower[2..].split('/').filter(|s| !s.is_empty());
        if let (Some(server), Some(share)) = (parts.next(), parts.next()) {
            return format!("//{server}/{share}");
        }
        return lower;
    }
    if lower.starts_with('/') {
        return "/".to_string();
    }
    String::new()
}

fn is_relative_path(normalized: &str) -> bool {
    let b = normalized.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return false;
    }
    if normalized.starts_with("//") || normalized.starts_with('/') {
        return false;
    }
    true
}

fn comps_after_root(path_norm: &str) -> Vec<&str> {
    let b = path_norm.as_bytes();
    let is_drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    let is_unc = path_norm.starts_with("//");
    if is_drive {
        path_norm[2..].split('/').filter(|s| !s.is_empty()).collect()
    } else if is_unc {
        let all: Vec<&str> = path_norm[2..].split('/').filter(|s| !s.is_empty()).collect();
        if all.len() >= 2 {
            all[2..].to_vec()
        } else {
            Vec::new()
        }
    } else {
        path_norm.split('/').filter(|s| !s.is_empty()).collect()
    }
}

fn file_name_only(path: &str) -> String {
    let disp = normalize_display(path);
    if disp.is_empty() {
        return String::new();
    }
    let mut base = disp.rsplit('/').next().unwrap_or("");
    let bb = base.as_bytes();
    if bb.len() >= 2 && bb[0].is_ascii_alphabetic() && bb[1] == b':' {
        base = &base[2..];
    }
    base.to_string()
}

/// Fallible pathtype conversion. `Full` and `None` always succeed;
/// `Relative` fails with [`PathtypeError::AcrossDrives`] when drive letters
/// or UNC hosts differ (UNC vs drive-letter counts as different), and with
/// [`PathtypeError::UnsavedHost`] when `host` has no parent directory.
/// Drive comparison is case-insensitive; all comparisons use
/// [`normalize_lexical`].
pub fn to_pathtype_result(
    path: &str,
    host: &std::path::Path,
    pathtype: Pathtype,
) -> Result<String, PathtypeError> {
    match pathtype {
        // Absolute output: a stored relative path is resolved against the
        // host folder first, so `Full` is never a silent no-op on one.
        Pathtype::Full => {
            if is_relative_path(&normalize_lexical(path)) && !normalize_lexical(path).is_empty() {
                if let Some(dir) = host.parent() {
                    if !dir.as_os_str().is_empty() {
                        let joined = dir.join(normalize_display(path)).to_string_lossy().into_owned();
                        return Ok(normalize_display(&joined));
                    }
                }
            }
            Ok(normalize_display(path))
        }
        Pathtype::None => Ok(file_name_only(path)),
        Pathtype::Relative => {
            let parent = host.parent();
            let parent = match parent {
                None => return Err(PathtypeError::UnsavedHost),
                Some(p) if p.as_os_str().is_empty() => return Err(PathtypeError::UnsavedHost),
                Some(p) => p,
            };
            let host_dir_norm = normalize_lexical(&parent.to_string_lossy());
            if host_dir_norm.is_empty() {
                return Err(PathtypeError::UnsavedHost);
            }
            let target_norm = normalize_lexical(path);
            if target_norm.is_empty() {
                return Ok(String::new());
            }
            let target_root = root_key(&target_norm);
            let host_root = root_key(&host_dir_norm);
            if target_root != host_root {
                if is_relative_path(&target_norm) {
                    return Ok(normalize_display(path));
                }
                return Err(PathtypeError::AcrossDrives);
            }
            let target_comps_norm = comps_after_root(&target_norm);
            let host_comps_norm = comps_after_root(&host_dir_norm);
            let mut k = 0usize;
            while k < target_comps_norm.len()
                && k < host_comps_norm.len()
                && target_comps_norm[k] == host_comps_norm[k]
            {
                k += 1;
            }
            // Display components mirror normalized ones 1:1 (case folding
            // cannot change component counts), so the display tail is reused
            // to preserve the stored casing in the output.
            let target_disp = normalize_display(path);
            let target_comps_disp = comps_after_root(&target_disp);
            debug_assert_eq!(target_comps_disp.len(), target_comps_norm.len());
            let mut parts: Vec<String> = Vec::new();
            for _ in 0..host_comps_norm.len().saturating_sub(k) {
                parts.push("..".to_string());
            }
            for c in target_comps_disp.iter().skip(k) {
                parts.push(c.to_string());
            }
            if parts.is_empty() {
                return Ok(file_name_only(path));
            }
            Ok(parts.join("/"))
        }
    }
}

/// Panic-free pathtype conversion. [`PathtypeError::AcrossDrives`] and
/// [`PathtypeError::UnsavedHost`] fall back to [`Pathtype::Full`]
/// (absolute, separator-normalized) so callers always get a usable path.
pub fn to_pathtype(path: &str, host: &std::path::Path, pathtype: Pathtype) -> String {
    match to_pathtype_result(path, host, pathtype) {
        Ok(s) => s,
        Err(_) => normalize_display(path),
    }
}

/// Case-insensitive name match: `*` matches any run (including empty),
/// `?` matches exactly one char.
pub fn wildcard_match(name: &str, pattern: &str) -> bool {
    let n: Vec<char> = name.chars().flat_map(|c| c.to_lowercase()).collect();
    let p: Vec<char> = pattern.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut si = 0usize;
    let mut pi = 0usize;
    let mut star: Option<usize> = None;
    let mut match_idx = 0usize;
    while si < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[si]) {
            si += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            match_idx = si;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            match_idx += 1;
            si = match_idx;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// What kind of external file a [`ReferenceEntry`] points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    DwgXref,
    Image,
    Pdf,
}

/// Lifecycle state of a [`ReferenceEntry`].
///
/// `Stale` (file changed since load) and `Orphaned` (definition with no
/// referencing entity) are detected in Task 8 — [`collect_entries`] never
/// produces them yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefStatus {
    Loaded,
    Unloaded,
    NotFound,
    Failed,
    Stale,
    /// A nested entry whose host failed (distinct from [`RefStatus::Unreferenced`).
    Orphaned,
    /// A definition with no placed instances (e.g. an image definition no
    /// entity references). Listed so it can be managed, never merged.
    Unreferenced,
}

/// How a DWG xref attaches: full re-export (`Attach`) vs local-only (`Overlay`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefType {
    Attach,
    Overlay,
}

/// One external reference in the XREF manager's unified list.
///
/// `key` is the block-record handle (DWG xref) or definition-object handle
/// (image / PDF) for top-level entries — stable across renames, so
/// [`renamed`](Self::renamed) keeps it. Nested child entries (enumerated from
/// a host file, never merged) use [`child_key`] — an FNV-1a namespaced hash
/// of `(parent_key, name, saved_path)` — so a foreign handle that collides
/// with a host handle can never alias a host row. `saved_path` is the raw
/// stored string verbatim, never synthesized; `found_at` is where it actually
/// resolved, if anywhere.
///
/// `parent_key` is `None` for roots and `Some(host_key)` for nested children.
/// The palette's refresh path keys rows off `(key, saved_path)` — both sides
/// go through `collect_entries`, so the same derivation applies everywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceEntry {
    pub key: u64,
    pub name: String,
    pub kind: RefKind,
    pub ref_type: RefType,
    pub status: RefStatus,
    pub size_bytes: Option<u64>,
    pub modified: Option<std::time::SystemTime>,
    pub saved_path: String,
    pub found_at: Option<String>,
    pub parent_key: Option<u64>,
}

impl ReferenceEntry {
    pub fn new(key: u64, name: impl Into<String>, kind: RefKind) -> Self {
        Self {
            key,
            name: name.into(),
            kind,
            ref_type: RefType::Attach,
            status: RefStatus::NotFound,
            size_bytes: None,
            modified: None,
            saved_path: String::new(),
            found_at: None,
            parent_key: None,
        }
    }

    /// Same entry under a new name — the key is untouched.
    pub fn renamed(&self, name: impl Into<String>) -> Self {
        Self {
            key: self.key,
            name: name.into(),
            ..self.clone()
        }
    }
}

/// Bind-style symbol name for a nested xref block: the industry-standard
/// CAD bind scheme inserts `$0$` separators, so `PLAN` → `DETAIL` → `WALLS`
/// reads transitively.
pub fn bind_symbol(parent: &str, child: &str, sym: &str) -> String {
    format!("{parent}$0${child}$0${sym}")
}
/// [`bind_symbol`] for the two-level case, bumping the `$N$` counter past
/// every collision in `taken` (`PLAN$0$WALLS` taken → `PLAN$1$WALLS`).
///
/// Always terminates: `taken` holds N names, so one of the N+1 candidates
/// `0..=N` is necessarily free (pigeonhole) — no counter cap needed.
pub fn bind_symbol_taken(parent: &str, sym: &str, taken: &[impl AsRef<str>]) -> String {
    for n in 0..=taken.len() as u32 {
        let candidate = format!("{parent}${n}${sym}");
        if !taken.iter().any(|t| t.as_ref() == candidate) {
            return candidate;
        }
    }
    // Unreachable by the argument above; kept total instead of panicking.
    format!("{parent}$4294967295${sym}")
}

/// Stable key for a nested child entry.
///
/// Top-level entries keep the host handle value unchanged. Nested children
/// come from foreign files whose handle values can collide with host handles,
/// so they are namespaced via FNV-1a over `(parent_key, name, saved_path)`
/// with the top bit forced set (`0x8000_0000_0000_0000`). The high bit is the
/// namespace tag: host handles (sequentially allocated, nowhere near 2^63)
/// never have it set, so a nested key can never alias a host key — in the
/// stat cache, the unload set's raw views, or anywhere else a bare `u64`
/// travels. FNV-1a is chosen over `DefaultHasher` deliberately: the algorithm
/// is fixed, so keys are stable across runs and toolchain versions (a
/// `DefaultHasher` key would become a persistence-format hazard the moment
/// unload state is ever persisted). Same derivation is used everywhere
/// `collect_entries` output is consumed, so palette `(key, saved_path)` rows
/// keep resolving.
pub fn child_key(parent_key: u64, name: &str, saved_path: &str) -> u64 {
    // FNV-1a/64 (offset 14695981039346656037, prime 1099511628211).
    let mut h: u64 = 14695981039346656037;
    for b in parent_key
        .to_le_bytes()
        .iter()
        .chain(name.as_bytes())
        .chain([0u8].iter())
        .chain(saved_path.as_bytes())
    {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h | 0x8000_0000_0000_0000
}

/// Pure status decision for one entry (no filesystem).
///
/// - `resolved` is the stat outcome (`Loaded`/`NotFound`/`Failed`; `Unloaded`
///   never reaches here — it is decided before stat).
/// - `live` is the current mtime, `cached` the load-time mtime from the
///   previous refresh (`None` on first call → no `Stale` on bootstrap).
///   The comparison is absolute in both directions: a file restored from
///   backup (live < cached) changed content just as much as an edited one.
/// - `parent_failed` is true for nested children whose host entry is
///   `NotFound`/`Failed` → `Loaded` is overridden to `Orphaned`.
///
/// The 1s slack absorbs filesystem timestamp granularity so a file that did
/// not actually change is not flagged `Stale`. Conversely, changes landing
/// between document open and the first refresh are invisible that session —
/// the baseline only exists from the first stat on (no file watcher in v1).
pub fn decide_status(
    resolved: RefStatus,
    live: Option<std::time::SystemTime>,
    cached: Option<std::time::SystemTime>,
    parent_failed: bool,
) -> RefStatus {
    if parent_failed && resolved == RefStatus::Loaded {
        return RefStatus::Orphaned;
    }
    if resolved == RefStatus::Loaded {
        if let (Some(l), Some(c)) = (live, cached) {
            let delta = l.duration_since(c).ok().or_else(|| c.duration_since(l).ok());
            if delta.is_some_and(|d| d > std::time::Duration::from_secs(1)) {
                return RefStatus::Stale;
            }
        }
    }
    resolved
}

/// Session set of unloaded reference keys.
///
/// Owns the `HashSet<UnloadKey>` that `collect_entries` takes as `unloaded`.
/// Task 8b wires session state; the set itself lives here so both sides
/// share one owner.
///
/// Keys are domain-tagged: [`UnloadKey::Direct`] wraps a top-level handle
/// value (nested children are never unloadable on their own) while
/// [`UnloadKey::Nested`] wraps a nested [`child_key`] hash. The tag keeps a
/// nested hash from ever aliasing a host handle with the same numeric value.
/// NOTE (nested-key invariant): only [`add_key`](UnloadSet::add_key) may
/// insert `Nested` keys and only [`contains_key`](UnloadSet::contains_key) /
/// [`is_unloaded_key`](UnloadSet::is_unloaded_key) may query them — every op
/// site guards nested rows before touching session state, so the untagged
/// `add`/`contains`/`is_unloaded`/`remove` helpers below only ever see
/// direct keys. New code must use the `_key` variants for nested entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnloadKey {
    Direct(u64),
    Nested(u64),
}

#[derive(Debug, Clone, Default)]
pub struct UnloadSet(pub std::collections::HashSet<UnloadKey>);

impl UnloadSet {
    pub fn add(&mut self, key: u64) {
        self.0.insert(UnloadKey::Direct(key));
    }
    pub fn add_key(&mut self, key: UnloadKey) {
        self.0.insert(key);
    }
    pub fn remove(&mut self, key: &u64) {
        self.0.remove(&UnloadKey::Direct(*key));
    }
    pub fn remove_key(&mut self, key: &UnloadKey) {
        self.0.remove(key);
    }
    pub fn contains(&self, key: &u64) -> bool {
        self.0.contains(&UnloadKey::Direct(*key))
    }
    pub fn contains_key(&self, key: &UnloadKey) -> bool {
        self.0.contains(key)
    }
    pub fn is_unloaded(&self, key: u64) -> bool {
        self.0.contains(&UnloadKey::Direct(key))
    }
    pub fn is_unloaded_key(&self, key: UnloadKey) -> bool {
        self.0.contains(&key)
    }
    pub fn as_set(&self) -> &std::collections::HashSet<UnloadKey> {
        &self.0
    }
}

/// Load-time mtimes per reference key, populated on first refresh.
///
/// `Stale` is detectable only thereafter: an empty cache means bootstrap, so
/// `decide_status` never reports `Stale` without a cached baseline. Reload
/// clears the key (fresh baseline on next refresh).
///
/// KEY NAMESPACE: keys are host-handle values for direct entries and
/// high-bit-tagged [`child_key`] hashes for nested children (see
/// [`child_key`]: the tag bit keeps the two spaces disjoint, so sharing one
/// `u64` map is sound — the same discipline as [`UnloadKey`], encoded in the
/// key itself because every call site here already threads bare `u64`s).
/// Never insert a foreign handle that is neither.
#[derive(Debug, Clone, Default)]
pub struct RefStatCache(pub std::collections::HashMap<u64, std::time::SystemTime>);

impl RefStatCache {
    pub fn insert(&mut self, key: u64, mtime: std::time::SystemTime) {
        self.0.insert(key, mtime);
    }
    pub fn get(&self, key: &u64) -> Option<std::time::SystemTime> {
        self.0.get(key).copied()
    }
    pub fn remove(&mut self, key: &u64) {
        self.0.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_display, normalize_lexical, to_pathtype, to_pathtype_result, wildcard_match, Pathtype, PathtypeError};
    use super::{bind_symbol, bind_symbol_taken, RefKind, ReferenceEntry};
    use super::{child_key, decide_status, RefStatCache, RefStatus, UnloadSet};

    #[test]
    fn child_key_differs_from_colliding_host_key() {
        // Crafted collision: nested foreign handle 42 equals a host handle 42.
        // Namespaced derivation must not alias the host row.
        let host_key = 42u64;
        let nested = child_key(100, "DETAIL", "refs/detail.dwg");
        assert_ne!(nested, host_key);
        // High-bit namespace tag: host handles never carry it.
        assert_ne!(nested & 0x8000_0000_0000_0000, 0);
        // Stable: same inputs → same key; different parent → different key.
        assert_eq!(nested, child_key(100, "DETAIL", "refs/detail.dwg"));
        assert_ne!(nested, child_key(101, "DETAIL", "refs/detail.dwg"));
    }

    #[test]
    fn normalize_is_idempotent() {
        // normalize(normalize(x)) == normalize(x) across drives, UNC,
        // relative, dot-segments, and empty inputs.
        let cases = [
            "C:\\Host\\PLAN.dwg",
            "c:/host/./plan.dwg",
            "//SERVER/Share/../Share/f.dwg",
            "//s/sh/../../x.dwg",
            "rel/../../x.dwg",
            "c:/a/../../b.dwg",
            "a//b\\\\c.dwg",
            "../up/ref.dwg",
            "",
            ".",
        ];
        for c in cases {
            let once = normalize_lexical(c);
            assert_eq!(normalize_lexical(&once), once, "idempotence for {c:?}");
            assert_eq!(normalize_display(&once), once, "display-stable for {c:?}");
        }
        // UNC climbs clamp at the share root instead of producing garbage.
        assert_eq!(normalize_lexical("//s/sh/../../x.dwg"), "//s/sh/x.dwg");
    }

    #[test]
    fn relative_roundtrip_invariant() {
        // The Relative branch's real contract: converting an absolute path to
        // Relative and resolving it against the same host must recover the
        // normalized absolute path.
        let host = std::path::Path::new("C:/Drawings/host.dwg");
        for target in [
            "C:/Drawings/refs/plan.dwg",
            "C:/Drawings/plan.dwg",
            "C:/Lib/detail.dwg",
        ] {
            let rel = to_pathtype(target, host, Pathtype::Relative);
            let back = std::path::Path::new("C:/Drawings").join(&rel);
            assert_eq!(
                normalize_lexical(&back.to_string_lossy()),
                normalize_lexical(target),
                "roundtrip for {target:?} via {rel:?}"
            );
        }
    }

    #[test]
    fn decide_status_backwards_mtime_is_stale() {
        // A file restored from backup (live < cached) changed content just
        // as much as an edited one: absolute difference, both directions.
        use std::time::{Duration, UNIX_EPOCH};
        let cached = UNIX_EPOCH + Duration::from_secs(1_000);
        let older = cached - Duration::from_secs(30);
        assert_eq!(
            decide_status(RefStatus::Loaded, Some(older), Some(cached), false),
            RefStatus::Stale
        );
    }

    #[test]
    fn decide_status_stale_needs_slack() {
        use std::time::{Duration, UNIX_EPOCH};
        let cached = UNIX_EPOCH + Duration::from_secs(1_000);
        // +0.5s: within 1s slack → stays Loaded.
        let live = cached + Duration::from_millis(500);
        assert_eq!(
            decide_status(RefStatus::Loaded, Some(live), Some(cached), false),
            RefStatus::Loaded
        );
        // +2s: beyond slack → Stale.
        let live2 = cached + Duration::from_secs(2);
        assert_eq!(
            decide_status(RefStatus::Loaded, Some(live2), Some(cached), false),
            RefStatus::Stale
        );
        // No cached baseline (first refresh) → never Stale on bootstrap.
        assert_eq!(
            decide_status(RefStatus::Loaded, Some(live2), None, false),
            RefStatus::Loaded
        );
    }

    #[test]
    fn decide_status_orphaned_overrides_loaded_only() {
        use std::time::{Duration, UNIX_EPOCH};
        let t = Some(UNIX_EPOCH + Duration::from_secs(1_000));
        assert_eq!(
            decide_status(RefStatus::Loaded, t, t, true),
            RefStatus::Orphaned
        );
        // Non-Loaded nested states survive the parent override.
        assert_eq!(
            decide_status(RefStatus::NotFound, t, t, true),
            RefStatus::NotFound
        );
        assert_eq!(
            decide_status(RefStatus::Failed, t, t, true),
            RefStatus::Failed
        );
    }

    #[test]
    fn unload_set_add_remove_contains() {
        let mut s = UnloadSet::default();
        assert!(!s.is_unloaded(7));
        s.add(7);
        assert!(s.is_unloaded(7));
        assert!(s.contains(&7));
        s.remove(&7);
        assert!(!s.is_unloaded(7));
    }

    #[test]
    fn unload_set_namespaces_direct_vs_nested() {
        use super::UnloadKey;
        // Crafted collision: host handle 42 vs nested child hash 42 must not
        // cross-trigger — the domain tag keeps the two spaces apart.
        let mut s = UnloadSet::default();
        s.add(42);
        assert!(s.is_unloaded(42));
        assert!(!s.contains_key(&UnloadKey::Nested(42)));
        assert!(!s.is_unloaded_key(UnloadKey::Nested(42)));
        let mut t = UnloadSet::default();
        t.add_key(UnloadKey::Nested(42));
        assert!(!t.is_unloaded(42));
        assert!(!t.contains(&42));
        assert!(t.is_unloaded_key(UnloadKey::Nested(42)));
    }

    #[test]
    fn ref_stat_cache_roundtrip() {
        use std::time::{Duration, UNIX_EPOCH};
        let mut c = RefStatCache::default();
        let t = UNIX_EPOCH + Duration::from_secs(5);
        c.insert(9, t);
        assert_eq!(c.get(&9), Some(t));
    }

    #[test]
    fn lexical_normalize_missing_file_no_fs_touch() {
        let a = normalize_lexical("C:\\Host\\PLAN.dwg");
        let b = normalize_lexical("c:/host/./plan.dwg");
        assert_eq!(a, b);
        let c = normalize_lexical("C:\\NoSuchDir\\Sub\\..\\REF.DWG");
        let d = normalize_lexical("c:\\nosuchdir\\ref.dwg");
        assert_eq!(c, d);
    }

    #[test]
    #[cfg(windows)]
    fn lexical_normalize_exact_forms() {
        assert_eq!(normalize_lexical("C:\\Host\\PLAN.dwg"), "c:/host/plan.dwg");
        assert_eq!(
            normalize_lexical("C:\\NoSuchDir\\Sub\\..\\REF.DWG"),
            "c:/nosuchdir/ref.dwg"
        );
        assert_eq!(
            normalize_lexical("//SERVER/Share/./File.DWG"),
            "//server/share/file.dwg"
        );
        assert_eq!(normalize_lexical("c:/a/../../b.dwg"), "c:/b.dwg");
        assert_eq!(normalize_lexical("rel/../../x.dwg"), "../x.dwg");
        assert_eq!(normalize_lexical("a//b\\\\c.dwg"), "a/b/c.dwg");
        assert_eq!(normalize_lexical("a/b/"), "a/b");
        assert_eq!(normalize_lexical(""), "");
        assert_eq!(normalize_lexical("."), "");
    }

    // Deferred byte-level roundtrip probe (SPIKE1 / Task 1 carryover):
    // temp-dir only, nothing added to the repo. Builds an in-memory document
    // carrying each reference kind, writes it out, re-reads, and asserts
    // every SPIKE1 cell survived the round trip.
    #[cfg(not(target_arch = "wasm32"))]
    fn roundtrip_probe(ext: &str) {
        use acadrust::objects::{ImageDefinition, ObjectType, UnderlayDefinition};
        use acadrust::tables::BlockRecord;
        use acadrust::CadDocument;

        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_probe_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("probe dir");

        let mut doc = CadDocument::new();

        // Cell 1: xref block record — is_xref flag + absolute path.
        let xref_abs = dir.join("PROBE_REF.dwg").to_string_lossy().into_owned();
        let mut br = BlockRecord::new("PROBE_REF");
        br.flags.is_xref = true;
        br.xref_path = xref_abs.clone();
        doc.block_records.add(br).expect("add xref block record");

        // Cell 2: raster image definition with an absolute file path.
        let img_handle = doc.allocate_handle();
        let mut img_def =
            ImageDefinition::with_dimensions(r"C:\Probe\IMG.png", 64u32, 64u32);
        img_def.handle = img_handle;
        img_def.is_loaded = true;
        doc.objects
            .insert(img_handle, ObjectType::ImageDefinition(img_def));

        // Cell 3: PDF underlay definition with an absolute file path.
        let und_handle = doc.allocate_handle();
        let mut und_def = UnderlayDefinition::pdf(r"C:\Probe\DOC.pdf", "1");
        und_def.handle = und_handle;
        doc.objects
            .insert(und_handle, ObjectType::UnderlayDefinition(und_def));

        // Cell 4: the retain flag itself ($VISRETAIN).
        doc.header.retain_xref_visibility = true;

        let bytes = crate::io::save_to_bytes(&doc, ext, doc.version)
            .expect("probe save");
        std::fs::write(dir.join(format!("probe.{ext}")), &bytes).expect("probe write");

        let back = crate::io::load_bytes(&format!("probe.{ext}"), bytes).expect("probe reload");

        let br2 = back
            .block_records
            .iter()
            .find(|b| b.name.eq_ignore_ascii_case("PROBE_REF"))
            .expect("xref block record round-trips");
        assert!(br2.flags.is_xref, "is_xref flag round-trips");
        assert_eq!(br2.xref_path, xref_abs, "abs xref path round-trips");

        let img_back = back
            .objects
            .values()
            .find_map(|o| match o {
                ObjectType::ImageDefinition(d) => Some(d),
                _ => None,
            })
            .expect("image definition round-trips");
        assert_eq!(img_back.file_name, r"C:\Probe\IMG.png");

        let und_back = back
            .objects
            .values()
            .find_map(|o| match o {
                ObjectType::UnderlayDefinition(d) => Some(d),
                _ => None,
            })
            .expect("underlay definition round-trips");
        assert_eq!(und_back.file_path, r"C:\Probe\DOC.pdf");

        assert!(
            back.header.retain_xref_visibility,
            "retain_xref_visibility round-trips as true"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn probe_roundtrip_dwg() {
        roundtrip_probe("dwg");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn probe_roundtrip_dxf() {
        roundtrip_probe("dxf");
    }

    #[test]
    fn pathtype_full_relative_none_roundtrip() {
        let host = std::path::Path::new("C:/Drawings/host.dwg");
        let full = to_pathtype("C:/Drawings/refs/plan.dwg", host, Pathtype::Full);
        assert_eq!(full, "C:/Drawings/refs/plan.dwg");
        let rel = to_pathtype("C:/Drawings/refs/plan.dwg", host, Pathtype::Relative);
        assert_eq!(rel, "refs/plan.dwg");
        let none = to_pathtype("C:/Drawings/refs/plan.dwg", host, Pathtype::None);
        assert_eq!(none, "plan.dwg");
    }
    #[test]
    fn relative_across_drives_errors() {
        let host = std::path::Path::new("C:/Drawings/host.dwg");
        let err = to_pathtype_result("D:/Lib/plan.dwg", host, Pathtype::Relative);
        assert_eq!(err, Err(PathtypeError::AcrossDrives));
    }
    #[test]
    fn wildcard_name_match_case_insensitive() {
        assert!(wildcard_match("PLAN-East", "plan-*"));
        assert!(wildcard_match("A1", "a?"));
        assert!(!wildcard_match("A12", "a?"));
    }

    #[test]
    fn entry_key_stable_across_rename() {
        let a = ReferenceEntry::new(101, "PLAN", RefKind::DwgXref);
        let b = a.renamed("PLAN-EAST");
        assert_eq!(a.key, b.key);
        assert_eq!(b.name, "PLAN-EAST");
    }
    #[test]
    fn bind_chain_naming_transitive() {
        assert_eq!(bind_symbol("PLAN", "DETAIL", "WALLS"), "PLAN$0$DETAIL$0$WALLS");
        assert_eq!(bind_symbol_taken("PLAN", "WALLS", &["PLAN$0$WALLS"]), "PLAN$1$WALLS");
    }
}
