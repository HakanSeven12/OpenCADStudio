// OpenCADStudio hatch pattern catalog — built from `assets/patterns/OpenCADStudio.pat`.
//
// Each `PatternEntry` wraps a parsed PAT pattern with:
//   - `gpu`       — `HatchPattern::Pattern(families)` for the shader
//   - `pat_lines` — exact PAT line definitions used for DXF export

use std::sync::OnceLock;

use crate::io::patterns::PatLineDef;
use crate::scene::model::hatch_model::{HatchPattern, PatFamily};
use codec::entities::{HatchPattern as DxfPattern, HatchPatternLine};
use codec::types::Vector2;

// ── Public types ──────────────────────────────────────────────────────────

pub struct PatternEntry {
    pub name: String,
    pub description: String,
    /// GPU-ready pattern for the shader.
    pub gpu: HatchPattern,
    /// Exact PAT line families (used for DXF export).
    pub pat_lines: Vec<PatLineDef>,
}

// ── Catalog ───────────────────────────────────────────────────────────────

static CATALOG: OnceLock<Vec<PatternEntry>> = OnceLock::new();

pub fn catalog() -> &'static [PatternEntry] {
    CATALOG.get_or_init(build_catalog)
}

pub fn find(name: &str) -> Option<&'static PatternEntry> {
    catalog().iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

// ── DXF export ────────────────────────────────────────────────────────────

pub fn build_dxf_pattern(entry: &PatternEntry) -> DxfPattern {
    let mut pat = DxfPattern::new(&entry.name);
    pat.description = entry.description.clone();
    for ln in &entry.pat_lines {
        let angle_rad = (ln.angle_deg as f64).to_radians();
        // The catalog stores the step (dx = shift along the line, dy =
        // perpendicular spacing) in the pattern LINE-LOCAL frame. The DWG
        // `HatchPatternLine.offset` is a WORLD-space vector — that is how real
        // files store it and how `family_from_stored_line` reads it back (it
        // inverse-rotates by the line angle). Emitting the raw local step here
        // made the reader recover a rotated, too-dense spacing (e.g. ANSI31 at
        // 45° collapsed 3.175 → 2.245). Rotate local → world by the line angle
        // so the offset is format-correct and round-trips to the exact spacing.
        let (ca, sa) = (angle_rad.cos(), angle_rad.sin());
        let (ldx, ldy) = (ln.dx as f64, ln.dy as f64);
        pat.lines.push(HatchPatternLine {
            angle: angle_rad,
            base_point: Vector2::new(ln.x0 as f64, ln.y0 as f64),
            offset: Vector2::new(ldx * ca - ldy * sa, ldx * sa + ldy * ca),
            dash_lengths: ln.dashes.iter().map(|&d| d as f64).collect(),
        });
    }
    pat
}

// ── Builder ───────────────────────────────────────────────────────────────

fn build_catalog() -> Vec<PatternEntry> {
    let mut entries = vec![PatternEntry {
        name: "SOLID".into(),
        description: "Solid fill".into(),
        gpu: HatchPattern::Solid,
        pat_lines: vec![],
    }];

    for def in crate::io::patterns::catalog() {
        entries.push(PatternEntry {
            name: def.name.clone(),
            description: def.description.clone(),
            gpu: HatchPattern::Pattern(def.lines.iter().map(pat_line_to_family).collect()),
            pat_lines: def.lines.clone(),
        });
    }
    entries
}

fn pat_line_to_family(ln: &PatLineDef) -> PatFamily {
    PatFamily {
        angle_deg: ln.angle_deg,
        x0: ln.x0,
        y0: ln.y0,
        dx: ln.dx,
        dy: ln.dy,
        dashes: ln.dashes.clone(),
    }
}
