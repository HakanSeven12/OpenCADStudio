//! OpenCADStudio hatch pattern catalog — loaded from `assets/patterns/OpenCADStudio.pat`.
//!
//! PAT line format:  `angle, x0, y0, dx, dy [, dash1, dash2, ...]`
//!   - `angle`      line direction in degrees
//!   - `x0, y0`     origin of the first line in the family
//!   - `dx, dy`     step vector to the next parallel line
//!   - `dash…`      positive = dash length, negative = gap length (omit = solid)

use std::sync::OnceLock;

const PAT_SRC: &str = include_str!("../../assets/patterns/OpenCADStudio.pat");

// ── Public types ──────────────────────────────────────────────────────────

/// One line family inside a hatch pattern.
#[derive(Clone, Debug)]
pub struct PatLineDef {
    /// Line direction in degrees.
    pub angle_deg: f32,
    /// Origin of the first line in the family.
    pub x0: f32,
    pub y0: f32,
    /// Step vector to the next parallel line.
    pub dx: f32,
    pub dy: f32,
    /// Dash/gap sequence (positive = dash, negative = gap, empty = solid).
    pub dashes: Vec<f32>,
}

/// One parsed hatch pattern.
#[derive(Clone, Debug)]
pub struct PatternDef {
    pub name: String,
    pub description: String,
    pub lines: Vec<PatLineDef>,
}

// ── Static catalog ────────────────────────────────────────────────────────

static CATALOG: OnceLock<Vec<PatternDef>> = OnceLock::new();

/// All parsed patterns, in file order: the bundled catalog, then the user's
/// own `.pat` files.
pub fn catalog() -> &'static [PatternDef] {
    CATALOG.get_or_init(|| {
        parse(&format!(
            "{PAT_SRC}\n{}",
            user_definitions("patterns", "pat")
        ))
    })
}

/// The text of every `*.<ext>` file in `<config>/<folder>`, where users drop
/// their own hatch patterns and linetypes. Bundled names come first, so a
/// user file adds definitions rather than replacing the built-in ones.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn user_definitions(folder: &str, ext: &str) -> String {
    let Some(dir) = crate::config::config_dir().map(|dir| dir.join(folder)) else {
        return String::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return String::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        })
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The web build has no user folder.
#[cfg(target_arch = "wasm32")]
pub(crate) fn user_definitions(_folder: &str, _ext: &str) -> String {
    String::new()
}

// ── Geometry helper ───────────────────────────────────────────────────────

// ── Parser ────────────────────────────────────────────────────────────────

fn parse(src: &str) -> Vec<PatternDef> {
    let mut result = Vec::new();
    let mut cur_name = String::new();
    let mut cur_desc = String::new();
    let mut cur_lines: Vec<PatLineDef> = Vec::new();
    let mut in_pattern = false;

    let flush = |name: &mut String,
                 desc: &mut String,
                 lines: &mut Vec<PatLineDef>,
                 result: &mut Vec<PatternDef>| {
        if !lines.is_empty() {
            result.push(PatternDef {
                name: std::mem::take(name),
                description: std::mem::take(desc),
                lines: std::mem::take(lines),
            });
        }
        *name = String::new();
        *desc = String::new();
    };

    for raw in src.lines() {
        let line = raw.trim();

        // Skip blanks and comments.
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if let Some(rest) = line.strip_prefix('*') {
            // Flush previous pattern.
            flush(&mut cur_name, &mut cur_desc, &mut cur_lines, &mut result);
            in_pattern = true;

            // Header: *NAME[,Description]
            if let Some((name, desc)) = rest.split_once(',') {
                cur_name = name.trim().to_string();
                cur_desc = desc.trim().to_string();
            } else {
                cur_name = rest.trim().to_string();
                cur_desc = String::new();
            }
        } else if in_pattern {
            // Pattern line: angle, x0, y0, dx, dy [, d1, d2, ...]
            // Strip inline comments.
            let data = line.split(';').next().unwrap_or("").trim();
            if data.is_empty() {
                continue;
            }

            let parts: Vec<f32> = data
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();

            if parts.len() >= 5 {
                cur_lines.push(PatLineDef {
                    angle_deg: parts[0],
                    x0: parts[1],
                    y0: parts[2],
                    dx: parts[3],
                    dy: parts[4],
                    dashes: parts[5..].to_vec(),
                });
            }
        }
    }

    // Flush last pattern.
    flush(&mut cur_name, &mut cur_desc, &mut cur_lines, &mut result);

    result
}
