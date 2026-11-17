//! A drawing whose scale list holds a NaN factor must still open and draw.
//!
//! DXF reals are parsed with `str::parse::<f64>`, which accepts `nan`, so a
//! damaged or hand-edited file can give a SCALE object NaN paper units, and
//! `Scale::factor()` then returns NaN. The scale list is sorted by factor, and
//! the status bar builds it on every redraw. Since Rust 1.81 `sort_by` may
//! panic when its comparison is not a total order, which
//! `partial_cmp(..).unwrap_or(Equal)` is not once a NaN is in a list of 21 or
//! more: the app then ended on the UI thread as soon as the drawing was shown.
#![cfg(not(target_arch = "wasm32"))]

use OpenCADStudio::scene::Scene;

/// The DXF text with the paper units (group 140) of the `which`-th SCALE
/// object replaced by `nan`.
fn with_nan_paper_units(dxf: &str, which: usize) -> String {
    let lines: Vec<&str> = dxf.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    let (mut seen, mut in_scale, mut done) = (0, false, false);
    for pair in lines.chunks(2) {
        let (code, value) = (pair[0], pair.get(1).copied().unwrap_or(""));
        if code.trim() == "0" {
            in_scale = value.trim() == "SCALE";
            seen += usize::from(in_scale);
        }
        out.push(code);
        if in_scale && !done && seen == which + 1 && code.trim() == "140" {
            out.push("nan");
            done = true;
        } else {
            out.push(value);
        }
    }
    assert!(done, "the DXF has no SCALE object {which} with paper units");
    out.join("\n") + "\n"
}

/// Both standard sets, as a drawing from the reference application carries
/// them: 36 scales, well over the 21 at which the sort can panic. The status
/// bar's list keeps only the family of the drawing's unit (18 scales in a
/// millimetre drawing, 20 in an inch one), too few to panic, but the old
/// comparison left it out of order around a NaN; it is checked in both.
#[test]
fn a_nan_scale_factor_read_from_dxf_does_not_panic_the_scale_list() {
    nan_scale_round_trip(4, "mm");
    nan_scale_round_trip(1, "in");
}

fn nan_scale_round_trip(insertion_units: i16, label: &str) {
    let mut scene = Scene::new();
    scene.document.header.insertion_units = insertion_units;
    scene.reset_scale_list_to(&[], true, true);
    let count = scene.scale_list().len();
    assert!(count > 21, "only {count} scales");

    let dir = std::env::temp_dir().join(format!("ocs-scale-nan-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let clean = dir.join("clean.dxf");
    OpenCADStudio::io::save(&scene.document, &clean).unwrap();
    let dxf = std::fs::read_to_string(&clean).unwrap();

    // Every position for the NaN: whether a given list panics depends on
    // where the NaN sits, so no single position is enough.
    for which in 0..count {
        let path = dir.join(format!("nan-{which}.dxf"));
        std::fs::write(&path, with_nan_paper_units(&dxf, which)).unwrap();
        let mut opened = Scene::new();
        opened.document = OpenCADStudio::io::load_file(&path).unwrap();

        let list = opened.scale_list();
        assert_eq!(list.len(), count, "NaN at {which}");
        let factors: Vec<f64> = list.iter().map(|(_, _, factor)| *factor).collect();
        assert_eq!(
            factors.iter().filter(|f| f.is_nan()).count(),
            1,
            "NaN at {which}: the NaN scale must be read and kept"
        );
        let finite: Vec<f64> = factors.iter().copied().filter(|f| !f.is_nan()).collect();
        assert!(
            finite.windows(2).all(|pair| pair[0] <= pair[1]),
            "NaN at {which}: the other scales stay in order: {finite:?}"
        );
        // What the status bar builds on every redraw.
        let picker = opened.cached_scale_picker_list();
        assert!(!picker.is_empty(), "NaN at {which}");
        let shown: Vec<f64> = picker
            .iter()
            .map(|(_, _, factor)| *factor)
            .filter(|f| !f.is_nan())
            .collect();
        assert!(
            shown.windows(2).all(|pair| pair[0] <= pair[1]),
            "NaN at {which}: the status bar's scales stay in order: {shown:?}"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}
