//! Command aliases — short abbreviations typed at the command line that expand
//! to a full command (e.g. `L` → `LINE`). The alias table is the single source
//! of truth: it is loaded from a user-editable `aliases.pgp` file at startup and
//! consulted before every command dispatch (see `resolve_alias`), so users can
//! add, remap, or delete aliases without touching the app.
//!
//! File format mirrors the long-standing plain-text `.pgp` convention so it is
//! familiar and hand-editable, stored as `ocad.pgp`:
//!
//! ```text
//! ; lines beginning with a semicolon are comments
//! L,        *LINE
//! CC,       *COPYCLIP
//! ```
//!
//! An alias is the token left of the comma; the command is the token right of
//! it, with an optional leading `*`. Both are matched case-insensitively and
//! stored uppercased. Native builds use `ocad.pgp`; web builds store the same
//! PGP text in `localStorage`.

use super::OpenCADStudio;
use rustc_hash::FxHashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// The shipped default alias file, embedded at compile time. Its aliases live in
/// `assets/ocad.pgp` — not in Rust — so the defaults are just data in the same
/// format the user edits. On first launch this text is written verbatim to the
/// user's config folder (comments and all) as the starting `ocad.pgp`; only that
/// user copy is read thereafter. This embedded copy is the first-run seed /
/// factory default.
const DEFAULT_ALIASES_PGP: &str = include_str!("../../assets/ocad.pgp");

/// Path to the user's alias file, `<config>/ocad.pgp`. `None` when the platform
/// config base can't be resolved (headless, no `HOME`).
#[cfg(not(target_arch = "wasm32"))]
fn alias_file_path() -> Option<PathBuf> {
    Some(crate::config::config_dir()?.join("ocad.pgp"))
}

/// Path to the marker recording which default-alias version this profile has
/// seen, `<config>/ocad.pgp.version`. Kept alongside the alias file; its absence
/// means "predates versioning" (treated as version 1).
#[cfg(not(target_arch = "wasm32"))]
fn alias_version_file_path() -> Option<PathBuf> {
    Some(crate::config::config_dir()?.join("ocad.pgp.version"))
}

/// Read the default-version a profile has seen. `0` when no marker exists —
/// indistinguishable from a first-run of an already-seeded file, so migrate only
/// neutral additions (never removals) in that case.
#[cfg(not(target_arch = "wasm32"))]
fn read_alias_version(path: &PathBuf) -> u32 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

/// Read the default-version a profile has seen from web `localStorage`.
#[cfg(target_arch = "wasm32")]
fn read_alias_version_web() -> u32 {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(WEB_ALIAS_VERSION_KEY).ok().flatten())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
const WEB_ALIAS_KEY: &str = "opencadstudio.aliases";

/// Version of the shipped default alias table. Bump this whenever the embedded
/// defaults add aliases or change a default target, so existing profiles can be
/// migrated forward (see `introduced_at` / `migrate_aliases`). Version 1 shipped
/// the pre-REDRAW table; version 2 introduced the REDRAW-family aliases; version 3
/// introduced HB (HATCHTOBACK).
const DEFAULT_ALIASES_VERSION: u32 = 3;

#[cfg(target_arch = "wasm32")]
const WEB_ALIAS_VERSION_KEY: &str = "opencadstudio.aliases.version";

/// The aliases introduced by a given version of the default table (1-indexed).
/// A profile that last saw version `V` receives each alias in versions `V+1..`
/// that it does not already define. Aliases introduced before `V`, even if the
/// user later removed them, are left alone so deliberate deletions survive.
fn introduced_at(version: u32) -> &'static [(&'static str, &'static str)] {
    match version {
        2 => &[
            ("R", "REDRAW"),
            ("RA", "REDRAWALL"),
            ("RE", "REGEN"),
            ("REA", "REGENALL"),
        ],
        3 => &[("HB", "HATCHTOBACK")],
        _ => &[],
    }
}

/// Fold the defaults introduced after `from_version` into `stored`, adding only
/// aliases the user does not already define (so overrides and custom targets
/// are preserved, and nothing already present is overwritten). Returns whether
/// any alias was added.
fn migrate_aliases(stored: &mut FxHashMap<String, String>, from_version: u32) -> bool {
    if from_version >= DEFAULT_ALIASES_VERSION {
        return false;
    }
    let mut changed = false;
    for version in from_version + 1..=DEFAULT_ALIASES_VERSION {
        for (alias, cmd) in introduced_at(version) {
            let key = alias.to_uppercase();
            if stored.contains_key(&key) {
                continue;
            }
            stored.insert(key, cmd.to_uppercase());
            changed = true;
        }
    }
    changed
}

/// Parse `.pgp` text into an `alias → command` map, both uppercased. Skips blank
/// lines and `;` comments; tolerates a leading `*` on the command and arbitrary
/// whitespace padding.
fn parse_pgp(body: &str) -> FxHashMap<String, String> {
    let mut map = FxHashMap::default();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let Some((alias, cmd)) = line.split_once(',') else {
            continue;
        };
        let alias = alias.trim().to_uppercase();
        let cmd = cmd.trim().trim_start_matches('*').trim().to_uppercase();
        if alias.is_empty() || cmd.is_empty() {
            continue;
        }
        map.insert(alias, cmd);
    }
    map
}

/// Serialize an `alias → command` map to `.pgp` text with a header and aligned
/// columns, sorted by alias for a stable, diffable file.
pub(super) fn to_pgp(map: &FxHashMap<String, String>) -> String {
    let mut rows: Vec<(&String, &String)> = map.iter().collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = String::from(
        "; OpenCADStudio command aliases.\n\
         ; Format:  ALIAS,*COMMAND   (lines starting with ';' are comments)\n\
         ; Edit here or via the ALIASEDIT command. One alias per line.\n\n",
    );
    for (alias, cmd) in rows {
        out.push_str(&format!("{:<10}*{}\n", format!("{alias},"), cmd));
    }
    out
}

/// The built-in defaults, parsed from the embedded `.pgp`. Used when no user
/// file exists yet (or can't be reached, e.g. wasm).
fn default_map() -> FxHashMap<String, String> {
    parse_pgp(DEFAULT_ALIASES_PGP)
}

/// Load the alias table at boot. Native reads `ocad.pgp`; web reads the same
/// text from `localStorage`. Missing or unavailable storage falls back to the
/// embedded defaults.
pub(super) fn load_aliases() -> FxHashMap<String, String> {
    // After any migration the caller's key differs; we shadow `path` here so
    // the version marker maps one-to-one with the alias file that produced it.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(path) = alias_file_path() else {
            return default_map();
        };
        return match std::fs::read_to_string(&path) {
            Ok(body) => {
                let mut map = parse_pgp(&body);
                let seen = alias_version_file_path()
                    .map(|vp| read_alias_version(&vp))
                    .unwrap_or(0);
                if migrate_aliases(&mut map, seen) {
                    // Persist the merged table, and only if that succeeds advance
                    // the version marker so a later launch retries the merge
                    // (otherwise the new aliases would be lost for good while the
                    // marker claimed they were delivered).
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if std::fs::write(&path, to_pgp(&map)).is_ok() {
                        if let Some(vp) = alias_version_file_path() {
                            let _ = std::fs::write(&vp, DEFAULT_ALIASES_VERSION.to_string());
                        }
                    }
                }
                map
            }
            Err(_) => {
                // No file yet — copy the shipped default file, best-effort, and
                // mark it as current so a later upgrade merges only new aliases.
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&path, DEFAULT_ALIASES_PGP);
                if let Some(vp) = alias_version_file_path() {
                    let _ = std::fs::write(&vp, DEFAULT_ALIASES_VERSION.to_string());
                }
                default_map()
            }
        };
    }

    #[cfg(target_arch = "wasm32")]
    {
        let Some(storage) =
            web_sys::window().and_then(|window| window.local_storage().ok().flatten())
        else {
            return default_map();
        };
        let stored = storage.get_item(WEB_ALIAS_KEY).ok().flatten();
        return match stored {
            Some(body) => {
                let mut map = parse_pgp(&body);
                let seen = read_alias_version_web();
                if migrate_aliases(&mut map, seen) {
                    // Only advance the version marker once the migrated table is
                    // actually stored, so a failed set_item leaves the marker
                    // behind and the merge is retried next launch.
                    if storage
                        .set_item(WEB_ALIAS_KEY, &to_pgp(&map))
                        .is_ok()
                    {
                        let _ = storage.set_item(
                            WEB_ALIAS_VERSION_KEY,
                            &DEFAULT_ALIASES_VERSION.to_string(),
                        );
                    }
                }
                map
            }
            None => {
                let _ = storage.set_item(
                    WEB_ALIAS_VERSION_KEY,
                    &DEFAULT_ALIASES_VERSION.to_string(),
                );
                default_map()
            }
        };
    }
}

/// Persist the alias table to native `ocad.pgp` or web `localStorage`.
pub(super) fn save_map(map: &FxHashMap<String, String>) -> std::io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(path) = alias_file_path() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        return std::fs::write(path, to_pgp(map));
    }

    #[cfg(target_arch = "wasm32")]
    {
        let Some(storage) =
            web_sys::window().and_then(|window| window.local_storage().ok().flatten())
        else {
            return Ok(());
        };
        storage
            .set_item(WEB_ALIAS_KEY, &to_pgp(map))
            .map_err(|_| std::io::Error::other("browser alias storage unavailable"))?;
        return Ok(());
    }
}

impl OpenCADStudio {
    /// Rewrite the leading command token through the alias table, leaving any
    /// arguments after the first whitespace untouched. Returns `None` when the
    /// verb is not an alias, so the caller passes the original string through
    /// unchanged. Keyed uppercase so lowercase aliases from scripts/plugins
    /// (which bypass the command line's verb-uppercasing) still resolve.
    pub(super) fn resolve_alias(&self, cmd: &str) -> Option<String> {
        if self.command_aliases.is_empty() {
            return None;
        }
        let (verb, rest) = match cmd.split_once(char::is_whitespace) {
            Some((v, r)) => (v, Some(r)),
            None => (cmd, None),
        };
        let canon = self.command_aliases.get(&verb.to_uppercase())?;
        Some(match rest {
            Some(r) => format!("{canon} {r}"),
            None => canon.clone(),
        })
    }

    /// Replace the alias table (from the editor), persist it, and mirror it into
    /// the command line so autocomplete reflects the edits immediately.
    pub(super) fn set_command_aliases(&mut self, map: FxHashMap<String, String>) {
        let _ = save_map(&map);
        self.command_line.command_aliases = map.clone();
        self.command_aliases = map;
    }

    /// Commit the alias-editor working rows to the table (Apply button): build a
    /// map from the rows, dropping incomplete ones, and persist + sync. Leaves
    /// the editor open so the user can keep editing.
    pub(super) fn apply_alias_editor_rows(&mut self) {
        let map: FxHashMap<String, String> = self
            .alias_editor_rows
            .iter()
            .filter(|(a, c)| !a.trim().is_empty() && !c.trim().is_empty())
            .map(|(a, c)| (a.trim().to_string(), c.trim().to_string()))
            .collect();
        self.set_command_aliases(map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> FxHashMap<String, String> {
        pairs
            .iter()
            .map(|(a, c)| (a.to_string(), c.to_string()))
            .collect()
    }

    /// An existing profile that predates the REDRAW and HB aliases gains exactly the
    /// new defaults, with every pre-existing mapping kept.
    #[test]
    fn migration_adds_new_defaults_preserving_existing() {
        let mut stored = map(&[("L", "LINE"), ("Z", "ZOOM"), ("HI", "HIDE")]);
        let changed = migrate_aliases(&mut stored, 0);
        assert!(changed, "migrating from v0 to v3 must add defaults");
        assert_eq!(stored.get("R").map(String::as_str), Some("REDRAW"));
        assert_eq!(stored.get("RA").map(String::as_str), Some("REDRAWALL"));
        assert_eq!(stored.get("RE").map(String::as_str), Some("REGEN"));
        assert_eq!(stored.get("REA").map(String::as_str), Some("REGENALL"));
        assert_eq!(stored.get("HB").map(String::as_str), Some("HATCHTOBACK"));
        // Existing entries must survive untouched.
        assert_eq!(stored.get("L").map(String::as_str), Some("LINE"));
        assert_eq!(stored.get("Z").map(String::as_str), Some("ZOOM"));
        assert_eq!(stored.get("HI").map(String::as_str), Some("HIDE"));
    }

    /// A user-defined mapping for an alias that collides with a new default is
    /// never overwritten by the migration.
    #[test]
    fn migration_preserves_user_override_of_new_alias() {
        let mut stored = map(&[("R", "REGEN")]);
        let changed = migrate_aliases(&mut stored, 0);
        assert!(changed);
        assert_eq!(
            stored.get("R").map(String::as_str),
            Some("REGEN"),
            "user's R=REGEN override must win over default R=REDRAW"
        );
    }

    /// A profile already at the current version is left untouched — deliberate
    /// removals are not resurrected on later launches.
    #[test]
    fn migration_is_noop_at_current_version() {
        let mut stored = map(&[("L", "LINE")]);
        let changed = migrate_aliases(&mut stored, DEFAULT_ALIASES_VERSION);
        assert!(
            !changed,
            "a current profile must not be re-migrated (preserves deletions)"
        );
        assert_eq!(stored.len(), 1);
        assert_eq!(stored.get("L").map(String::as_str), Some("LINE"));
    }

    /// Migrating a mid-range profile (one that saw v1) pulls in v2's and v3's
    /// additions, not hypothetical earlier ones.
    #[test]
    fn migration_from_version_one_adds_v2_and_v3_aliases() {
        let mut stored = map(&[("L", "LINE")]);
        let changed = migrate_aliases(&mut stored, 1);
        assert!(changed);
        assert_eq!(stored.get("R").map(String::as_str), Some("REDRAW"));
        assert_eq!(stored.get("RA").map(String::as_str), Some("REDRAWALL"));
        assert_eq!(stored.get("RE").map(String::as_str), Some("REGEN"));
        assert_eq!(stored.get("REA").map(String::as_str), Some("REGENALL"));
        assert_eq!(stored.get("HB").map(String::as_str), Some("HATCHTOBACK"));
    }

    /// Migrating a profile that saw v2 pulls in v3's HB addition only.
    #[test]
    fn migration_from_version_two_adds_v3_aliases() {
        let mut stored = map(&[("L", "LINE")]);
        let changed = migrate_aliases(&mut stored, 2);
        assert!(changed);
        assert_eq!(stored.get("R"), None);
        assert_eq!(stored.get("HB").map(String::as_str), Some("HATCHTOBACK"));
    }
}
