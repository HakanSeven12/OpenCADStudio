//! RED-TEAM: the web drag&drop name filter (`src/io/web_drop.rs`
//! `is_drawing_name`) cross-checked against the parse-worker extension gate
//! (`crates/ocs_web_worker/src/lib.rs:44-52`) it is supposed to pre-empt.
//!
//! A drop that passes the filter but fails the worker is a user-visible dead
//! end (file read, queued, then a parse error); a drop that fails the filter
//! for bytes the worker COULD parse is a silently ignored file. The two gates
//! are one-liners (`pub(crate)`/inline, invisible to integration tests), so
//! they are replicated VERBATIM here and fuzzed against each other; verdicts
//! are cross-checked with the real native loader `OpenCADStudio::io::load_bytes`
//! (src/io/mod.rs:722), which derives the format from the name exactly like
//! the worker.
#![cfg(not(target_arch = "wasm32"))]

const FIXTURE_DXF: &[u8] = include_bytes!("fixtures/embedded-image-print.dxf");

/// Verbatim copy of src/io/web_drop.rs (is_drawing_name).
fn is_drawing_name(name: &str) -> bool {
    matches!(
        name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase().as_str(),
        "dwg" | "dxf"
    )
}

/// Verbatim copy of the worker's gate (crates/ocs_web_worker/src/lib.rs:44-52).
fn worker_accepts(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or_default().to_lowercase();
    matches!(ext.as_str(), "dwg" | "dxf")
}

/// REGRESSION (fixed): the drop filter and the worker gate now share the
/// `rsplit('.').next()` convention, so a file literally named "DWG"/"dxf"
/// (no dot) is accepted by BOTH — previously the filter silently ignored
/// files the parser behind it would have opened.
#[test]
fn extensionless_dwg_named_files_are_dropped_but_parseable() {
    for name in ["DWG", "dwg", "Dwg", "DXF", "dxf"] {
        assert!(
            is_drawing_name(name),
            "{name:?}: drop filter accepts (worker convention)"
        );
        assert!(worker_accepts(name), "{name:?}: worker gate accepts");
        // The native loader shares the worker's convention: same bytes+name
        // it would reach the DXF/DWG reader (fixture is DXF, so the dxf
        // spellings truly parse; dwg spellings reach the DWG reader).
        let result = OpenCADStudio::io::load_bytes(name, FIXTURE_DXF.to_vec());
        assert_eq!(
            result.is_ok(),
            name.to_ascii_lowercase() == "dxf",
            "{name:?}: format is picked from the dot-less name itself"
        );
    }
}

/// For every name CONTAINING a dot the two gates agree: no dead end (filter
/// pass + worker refuse) and no silently ignored parseable file (filter
/// refuse + worker accept). Attacked with the hostile matrix.
#[test]
fn drop_filter_and_worker_gate_agree_on_dotted_names() {
    let hostile = [
        "plan.dwg",            // baseline
        ".DWG",                // uppercase dot-file: accepted by BOTH
        ".dwg",                // dot-file
        "x.dwg ",              // trailing space (legal on Linux)
        "x.dwg\u{a0}",         // trailing non-breaking space
        "x .dwg",              // space before extension
        "x.dwgx",              // near-miss extension
        "x.dwg.tar",           // double extension
        "x.DwF",               // Design Web Format: rejected
        "x.DXF",               // uppercase: accepted
        "x.dxf.",              // trailing dot: rejected
        ".",                   // only a dot
        "..",                  // only dots
        "…",                   // only an ellipsis char (not a dot)
        "",                    // empty (no dot — covered by the other test)
        "x.dωg",               // Greek omega lookalike
        "x.dＷg",               // fullwidth W (U+FF37)
        "x.dwg\n",             // trailing newline
        "\nx.dwg",             // leading newline
        "x.d\u{0130}f",        // dotted capital I (full lowercase => 2 chars)
        "x.dx\u{212a}",        // KELVIN SIGN lowercases to 'k'
        "con.dwg",             // Windows reserved stem
        "a.b.dxf",             // multi-dot
        "plán.dwg",            // unicode stem
        "図面.dxf",             // CJK stem
        "plan.sv$",            // native autosave copy: rejected on web
        "plan.bak",            // native backup: rejected on web
    ];
    for name in hostile {
        assert_eq!(
            is_drawing_name(name),
            worker_accepts(name),
            "drop filter and worker gate disagree on {name:?}"
        );
    }
    // Spot-check the interesting verdicts against the REAL loader (the
    // native `load_bytes` shares the rsplit('.') convention).
    assert!(is_drawing_name(".DWG") && worker_accepts(".DWG"));
    assert!(
        OpenCADStudio::io::load_bytes(".DWG", FIXTURE_DXF.to_vec()).is_err(),
        "dot-file '.DWG' is gated IN by both, then parsed as DWG (bytes decide)"
    );
    assert!(!is_drawing_name("x.dwg ") && !worker_accepts("x.dwg "));
    let err = OpenCADStudio::io::load_bytes("x.dwg ", FIXTURE_DXF.to_vec()).unwrap_err();
    assert!(err.contains("Unsupported file format"), "{err}");
    // No unicode fold can turn a non-ASCII extension into "dwg"/"dxf":
    // the only single-char case-folds onto ASCII letters are ASCII
    // themselves (plus KELVIN SIGN -> k, not in the set), so ascii-vs-full
    // lowercase cannot diverge for these two literals.
    assert!(!is_drawing_name("x.dx\u{212a}"));
    assert!(!worker_accepts("x.dx\u{212a}"));
}

/// Web/native divergence, real-code proof: `.sv$` and `.bak` files open
/// NATIVELY (sniffed, src/io/mod.rs:723) but are refused by BOTH web gates —
/// a user switching a crash-recovered `.sv$` from desktop to web gets a
/// console warning and nothing else.
#[test]
fn sv_dollar_and_bak_open_natively_but_are_refused_on_web() {
    assert!(!is_drawing_name("plan.sv$"));
    assert!(!worker_accepts("plan.sv$"));
    assert!(!is_drawing_name("plan.bak"));
    assert!(!worker_accepts("plan.bak"));
    // The native loader sniffs the same bytes under the same name.
    assert!(OpenCADStudio::io::load_bytes("plan.sv$", FIXTURE_DXF.to_vec()).is_ok());
    assert!(OpenCADStudio::io::load_bytes("plan.bak", FIXTURE_DXF.to_vec()).is_ok());
    // ...and refuses the web-refused near-misses identically.
    assert!(OpenCADStudio::io::load_bytes("plan.dwgx", FIXTURE_DXF.to_vec()).is_err());
}

/// The filter is name-only: a `.dwg`-named file whose bytes are DXF passes
/// both gates and dies in the DWG reader — the dead end the filter cannot
/// prevent (and cannot be expected to).
#[test]
fn filter_cannot_catch_bytes_format_mismatch() {
    assert!(is_drawing_name("actually-dxf.dwg"));
    assert!(worker_accepts("actually-dxf.dwg"));
    let error = OpenCADStudio::io::load_bytes("actually-dxf.dwg", FIXTURE_DXF.to_vec())
        .expect_err("DWG reader rejects DXF bytes");
    assert!(!error.contains("Unsupported file format"), "{error}");
}

/// Fuzz: over every short ASCII name, the ONLY divergences are the dot-less
/// exact "dwg"/"dxf" spellings pinned above — no other mismatch exists.
#[test]
fn exhaustive_short_name_agreement_except_extensionless_spelling() {
    let alphabet: Vec<u8> = (b'.'..=b'z').collect();
    let mut buffer = [0u8; 3];
    for &a in &alphabet {
        for &b in &alphabet {
            for &c in &alphabet {
                buffer = [a, b, c];
                let name = std::str::from_utf8(&buffer).unwrap();
                let filter = is_drawing_name(name);
                let worker = worker_accepts(name);
                if filter != worker {
                    // The one known class: dot-less exact spelling.
                    assert!(
                        !name.contains('.') && worker,
                        "unexpected divergence on {name:?}"
                    );
                }
            }
        }
    }
}
