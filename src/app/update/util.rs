//! Small pure helpers split out of `update.rs`.

use crate::scene::Scene;

/// Parse a scale string like "1:50" or "2:1" into (numerator, denominator).
/// Returns (1.0, 1.0) for "Fit" or unknown formats.
/// Sync the model-space annotation scale into its named drawing variable and
/// numeric header mirror before a save.
pub(super) fn sync_annotation_scale_header(scene: &mut Scene) {
    let anno = scene.annotation_scale;
    let value = if anno.abs() > 1e-9 {
        1.0 / anno as f64
    } else {
        1.0
    };
    let current = scene.document.header.current_annotation_scale.clone();
    let current_matches = scene.scale_list().into_iter().any(|(name, factor, _)| {
        name.eq_ignore_ascii_case(&current)
            && (factor - anno).abs() < 0.001 * anno.max(0.001)
    });
    let name = if current_matches {
        current
    } else {
        scene
            .scale_list()
            .into_iter()
            .find(|(_, factor, _)| (factor - anno).abs() < 0.001 * anno.max(0.001))
            .map(|(name, _, _)| name)
            .unwrap_or_else(|| format_annotation_scale_name(anno))
    };
    let hdr = &mut scene.document.header;
    hdr.current_annotation_scale = name.clone();
    hdr.annotation_scale_value = value;
    crate::io::set_drawing_variable(&mut scene.document, "CANNOSCALE", &name);
}

/// Format an annotation-scale multiplier as a ratio name: 50.0 -> "1:50",
/// 0.5 -> "2:1", 1.0 -> "1:1".
fn format_annotation_scale_name(anno: f32) -> String {
    if anno >= 1.0 {
        format!("1:{}", anno.round() as i64)
    } else if anno > 0.0 {
        format!("{}:1", (1.0 / anno).round() as i64)
    } else {
        "1:1".to_string()
    }
}

/// `<none>` / `<previous>` are pseudo-entries, not real page setups.
pub(super) fn is_special_entry(s: &str) -> bool {
    s == crate::ui::window::plot::SETUP_NONE || s == crate::ui::window::plot::SETUP_PREV
}

/// A page-setup list entry wrapped in `*…*` is a layout (its embedded page
/// setup), not a standalone named page setup.
pub(super) fn is_layout_entry(s: &str) -> bool {
    s.len() >= 2 && s.starts_with('*') && s.ends_with('*')
}

/// The layout name inside a `*name*` list entry.
pub(super) fn layout_entry_name(s: &str) -> &str {
    s.trim_start_matches('*').trim_end_matches('*')
}

/// Infer an A-series label when dimensions match; otherwise retain the size.
pub(super) fn paper_label_from_dims(w: f64, h: f64) -> (String, String) {
    use crate::io::paper_sizes::PaperSize;
    let orient = if w >= h { "Landscape" } else { "Portrait" };
    let (short, long) = if w <= h { (w, h) } else { (h, w) };
    let mut best = ("A4".to_string(), f64::INFINITY);
    for p in PaperSize::ALL {
        let (pw, ph) = p.dimensions_mm(); // portrait: pw < ph
        let err = (pw - short).abs() + (ph - long).abs();
        if err < best.1 {
            best = (p.label().to_string(), err);
        }
    }
    let label = if best.1 <= 2.0 {
        best.0
    } else {
        format!("{w:.2} × {h:.2} mm")
    };
    (label, orient.to_string())
}

pub(super) fn parse_plot_scale(s: &str) -> (f64, f64) {
    if s == "Fit" {
        return (1.0, 1.0);
    }
    if let Some((a, b)) = s.split_once(':') {
        let num: f64 = a.trim().parse().unwrap_or(1.0);
        let den: f64 = b.trim().parse().unwrap_or(1.0);
        if den > 0.0 {
            return (num, den);
        }
    }
    (1.0, 1.0)
}
