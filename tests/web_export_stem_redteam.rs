//! RED-TEAM: the web export filename pipeline
//! `pick_stl_path_owned`/`pick_step_path_owned` (src/io/stl.rs:102-104,
//! src/io/step.rs:258-260, wasm-only one-liners) fed by
//! `tab.current_path.file_stem()` (src/app/update/mod.rs:1310-1315) and
//! consumed by `sys::download_bytes` via `path.file_name()`
//! (src/app/update/file.rs:1717-1721, src/sys.rs:348-372).
//!
//! The wasm functions are cfg'd out on native, so the exact composition is
//! replicated here (it is pure `Path` semantics + `format!`) and attacked
//! with hostile current_paths. `download_bytes` itself only ever passes the
//! string to `a.set_download` — a plain JS property set that cannot panic in
//! Rust; browsers sanitize separators/control characters in the attribute.
#![cfg(not(target_arch = "wasm32"))]

/// Verbatim composition of the web flow: stem extraction (mod.rs:1310-1315)
/// -> virtual path (stl.rs:103 / step.rs:259) -> download name
/// (file.rs:1717-1721).
fn stl_download_name(current_path: Option<&std::path::Path>) -> String {
    let stem = current_path
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "drawing".into());
    let path = std::path::PathBuf::from(format!("{stem}.stl"));
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "export.stl".into())
}

fn step_download_name(current_path: Option<&std::path::Path>) -> String {
    let stem = current_path
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "drawing".into());
    let path = std::path::PathBuf::from(format!("{stem}.step"));
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "export.step".into())
}

/// Path separators coming from a hostile display name never survive into the
/// download: `file_stem()` already drops the directory part, and the
/// re-parsed virtual path's `file_name()` drops it again. No matter what
/// "current_path" a host page injects (the JS entry `ocs_web_drop_file`
/// accepts arbitrary name strings), the download filename cannot traverse.
#[test]
fn separators_and_dotdots_never_reach_the_download_name() {
    // Host page drops "a/b.dwg" — name with a forward separator.
    let name = stl_download_name(Some(std::path::Path::new("a/b.dwg")));
    assert_eq!(name, "b.stl");
    // Windows-style separator (web targets unix path rules: not a separator
    // there, but `file_name()` strips any leading "./" etc.; backslashes
    // survive as literal characters — browsers strip them in the download
    // attribute, and they cannot traverse OPFS because the name is never
    // used as a path, only as a display string + FNV-hash key input).
    let name = stl_download_name(Some(std::path::Path::new("..\\evil.dwg")));
    #[cfg(windows)]
    assert_eq!(name, "evil.stl");
    #[cfg(not(windows))]
    assert_eq!(name, "..\\evil.stl");
    // Pure ".." cannot be a stem: file_stem() of ".." is None -> "drawing".
    assert_eq!(stl_download_name(Some(std::path::Path::new(".."))), "drawing.stl");
    assert_eq!(step_download_name(Some(std::path::Path::new(".."))), "drawing.step");
    // No path at all (untitled tab).
    assert_eq!(stl_download_name(None), "drawing.stl");
    assert_eq!(step_download_name(None), "drawing.step");
    // Extension-less drawing name.
    assert_eq!(stl_download_name(Some(std::path::Path::new("Drawing1"))), "Drawing1.stl");
}

/// Quotes, newlines, control characters and unicode ride through as inert
/// string bytes: the Rust side does no filesystem or shell operation with
/// them (`download_bytes` builds a Blob and sets the `download` attribute —
/// src/sys.rs:356-369 — which cannot panic). NOTE: even an interior NUL
/// survives the whole Rust pipeline (std::path only rejects it at OS
/// boundaries, and there are none on web) — JS strings legally contain NUL,
/// and the browser sanitizes it in the download attribute. No injection, no
/// panic — verified by construction of the pipeline.
#[test]
fn quote_and_control_char_stems_stay_inert_strings() {
    for hostile in [
        "ev\"il", "ev'il", "line\nbreak", "tab\there", "cr\r lf", "nul\u{0}ish",
        "café", "図面", "..\\..\\evil", "con", "..",
    ] {
        let path = std::path::PathBuf::from(format!("{hostile}.dwg"));
        let name = stl_download_name(Some(&path));
        assert!(name.ends_with(".stl"), "{name:?}");
        let step_name = step_download_name(Some(&path));
        assert!(step_name.ends_with(".step"), "{step_name:?}");
    }
    // Pin the surprising-but-harmless case: an interior NUL is NOT stripped
    // anywhere in Rust; it would reach `set_download` as a JS string
    // containing U+0000 (legal), and the browser scrubs it.
    let name = stl_download_name(Some(std::path::Path::new("nul\u{0}ish.dwg")));
    assert_eq!(name, "nul\u{0}ish.stl");
}
