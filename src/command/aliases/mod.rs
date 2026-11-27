//! The command aliases this application ships, read from `assets/ocad.pgp`.
//! The user's own table, and resolving a typed alias, are in `app::alias`.

use rustc_hash::FxHashMap;

pub(crate) mod pgp;

/// Sorted factory-default rows for the ALIASEDIT reset action.
pub(crate) fn default_alias_rows() -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = default_map().into_iter().collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// The built-in defaults, parsed from the embedded `.pgp`. Used when no user
/// file exists yet (or can't be reached, e.g. wasm).
pub(crate) fn default_map() -> FxHashMap<String, String> {
    parse_pgp(DEFAULT_ALIASES_PGP)
}

/// Parse `.pgp` text into an `alias → command` map, both uppercased. Skips blank
/// lines and `;` comments; tolerates a leading `*` on the command and arbitrary
/// whitespace padding.
pub(crate) fn parse_pgp(body: &str) -> FxHashMap<String, String> {
    pgp::parse(body).into_iter().collect()
}

/// The shipped default alias file, embedded at compile time. Its aliases live in
/// `assets/ocad.pgp` — not in Rust — so the defaults are just data in the same
/// format the user edits. On first launch this text is written verbatim to the
/// user's config folder (comments and all) as the starting `ocad.pgp`; only that
/// user copy is read thereafter. This embedded copy is the first-run seed /
/// factory default.
pub(crate) const DEFAULT_ALIASES_PGP: &str = include_str!("../../../assets/ocad.pgp");
