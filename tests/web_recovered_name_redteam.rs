//! RED-TEAM: hostile inputs against the web autosave restore-name pipeline
//! (`src/io/web_autosave.rs` `recovered_name` / `newer_than_saved` /
//! `cap_evictions`) and the parse-worker extension gate it must satisfy
//! (`crates/ocs_web_worker/src/lib.rs`).
//!
//! `recovered_name` & co. are `pub(crate)` and OPFS is browser-only, so the
//! three pure helpers are copied VERBATIM below (marked) and their behaviour
//! pinned here; every claim that can be exercised through real shipped code
//! (`OpenCADStudio::io::load_bytes`, which derives the format from the name
//! exactly like the worker: `name.rsplit('.').next()`, src/io/mod.rs:722)
//! is tested against the real code.
#![cfg(not(target_arch = "wasm32"))]

const FIXTURE_DXF: &[u8] = include_bytes!("fixtures/embedded-image-print.dxf");

// ── Verbatim copies (pub(crate), invisible to integration tests) ───────────

/// Verbatim copy of src/io/web_autosave.rs `recovered_name` (incl. the
/// `RECOVERED_MARKER` const, inlined).
const RECOVERED_MARKER: &str = " (recovered)";
fn recovered_name(drawing: &str) -> String {
    let (head, ext) = match drawing.rsplit_once('.') {
        Some((head, ext)) if !ext.is_empty() => (head, ext.to_string()),
        // No dot (or a trailing one): the whole name is the stem.
        _ => (drawing, "dxf".to_string()),
    };
    // A hostile stored name must not smuggle path separators into the tab
    // title / download name: keep only the final component of the head.
    let head = head.rsplit(['/', '\\']).next().unwrap_or_default();
    // When the split consumed everything (".dxf") fall back to the whole
    // stored name, like `Path::file_stem`'s empty-stem fallback did.
    let mut stem = if head.is_empty() {
        drawing
    } else {
        head
    };
    // Idempotent: strip every trailing marker, then append exactly one.
    while let Some(stripped) = stem.strip_suffix(RECOVERED_MARKER) {
        stem = stripped;
    }
    format!("{stem}{RECOVERED_MARKER}.{ext}")
}

/// Verbatim copy of src/io/web_autosave.rs:52-57.
fn newer_than_saved(autosave_ms: f64, saved_ms: Option<f64>) -> bool {
    match saved_ms {
        Some(saved_ms) => autosave_ms > saved_ms,
        None => true,
    }
}

/// Verbatim copy of src/io/web_autosave.rs:39-43 + 71-84.
#[derive(Debug, Clone)]
struct WebAutosaveEntry {
    name: String,
    size: u64,
    modified_ms: f64,
}
const SNAPSHOT_CAP: usize = 16;
fn cap_evictions(
    mut entries: Vec<WebAutosaveEntry>,
    cap: usize,
) -> (Vec<WebAutosaveEntry>, Vec<String>) {
    entries.sort_by(|a, b| b.modified_ms.total_cmp(&a.modified_ms));
    let overflow = entries.len().saturating_sub(cap);
    let mut evicted: Vec<String> = entries
        .split_off(entries.len() - overflow)
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    evicted.reverse();
    (entries, evicted)
}

/// Verbatim copy of the worker's extension gate
/// (crates/ocs_web_worker/src/lib.rs:44-52): the restore name must pass this
/// or the drawing can NEVER be reopened — a restore that always fails is a
/// data-loss trap (the snapshot is only deleted on a successful open).
fn worker_accepts(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or_default().to_lowercase();
    matches!(ext.as_str(), "dwg" | "dxf")
}

fn entry(name: &str, modified_ms: f64) -> WebAutosaveEntry {
    WebAutosaveEntry {
        name: name.to_string(),
        size: 16,
        modified_ms,
    }
}

/// Every recovered name must keep an extension the parse worker accepts —
/// otherwise the offered snapshot can never be restored, only discarded.
#[test]
fn recovered_names_that_the_parse_worker_refuses() {
    // The extension is preserved verbatim: a ".sv$" name restores as ".sv$",
    // which the web worker rejects ("Unsupported file format: .sv$",
    // crates/ocs_web_worker/src/lib.rs:45) — while the NATIVE loader sniffs
    // the real format for sv$/bak (src/io/mod.rs:723) and opens it anyway.
    for input in ["plan.sv$", "plan.bak", "plan.dwf", "plan.dwgx", "pl.an"] {
        let output = recovered_name(input);
        assert!(
            !worker_accepts(&output),
            "recovered_name({input:?}) = {output:?} is refused by the parse worker"
        );
    }
    // Real-code proof of the web/native divergence: native `load_bytes`
    // derives the format from the name exactly like the worker, BUT sniffs
    // sv$/bak — the same bytes+name the worker refuses open natively.
    assert!(
        OpenCADStudio::io::load_bytes("plan (recovered).sv$", FIXTURE_DXF.to_vec()).is_ok(),
        "native loader sniffs sv$ and opens; only the web worker refuses"
    );
    // Non-sniffed unknown extensions are refused by BOTH loaders (the
    // replica gate mirrors the worker; load_bytes is the real thing).
    let err = OpenCADStudio::io::load_bytes("plan (recovered).dwf", FIXTURE_DXF.to_vec())
        .expect_err("real loader must refuse .dwf");
    assert!(err.contains("Unsupported file format"), "{err}");
}

/// FIXED (was BREAK, verified by reading + real-loader proof): a drawing
/// whose FILE NAME is exactly ".dxf" (a dot-file — accepted by BOTH the drop
/// filter and the worker, which split on the last dot) is serialized by
/// autosave as DXF (`source_is_dxf` falls through to
/// `dwg_source_version.is_none()` because `Path::extension(".dxf")` is
/// None). The old `Path::extension`-based `recovered_name` restored it as
/// ".dwg" — feeding DXF bytes to the DWG reader, so the restore could never
/// succeed and the snapshot was re-offered at every boot (it is deleted only
/// on a successful open). The extension split now follows the WORKER
/// convention (`name.rsplit('.').next()`), so ".dxf" restores as ".dxf".
#[test]
fn dotfile_dxf_restores_as_dxf_with_matching_bytes() {
    // 1. The drop filter (replicated verbatim from src/io/web_drop.rs)
    //    accepts ".dxf": rsplit_once('.') yields ("", "dxf").
    let dropped = ".dxf"
        .rsplit_once('.')
        .is_some_and(|(_, extension)| {
            matches!(extension.to_ascii_lowercase().as_str(), "dwg" | "dxf")
        });
    assert!(dropped, "drop filter accepts the dot-file '.dxf'");

    // 2. The worker gate (and the real loader) also parse ".dxf" as DXF.
    assert!(worker_accepts(".dxf"));
    let document = OpenCADStudio::io::load_bytes(".dxf", FIXTURE_DXF.to_vec())
        .expect("real loader opens '.dxf' as DXF");
    // ...and it is a DXF source, so `source_is_dxf` says DXF (the autosave
    // serializes DXF bytes).
    assert!(document.dwg_source_version.is_none());

    // 3. Worker-convention split: ".dxf" keeps "dxf" as its extension — the
    //    empty head falls back to the whole stored name as the stem.
    let restored = recovered_name(".dxf");
    assert_eq!(restored, ".dxf (recovered).dxf");

    // 4. Real loader: DXF bytes under the recovered name now go to the DXF
    //    reader and OPEN — the restore door is unjammed.
    assert!(worker_accepts(&restored));
    assert!(
        OpenCADStudio::io::load_bytes(&restored, FIXTURE_DXF.to_vec()).is_ok(),
        "DXF bytes under {restored:?} must parse"
    );
    // And the dot-file's DWG twin keeps its DWG extension the same way.
    assert_eq!(recovered_name(".dwg"), ".dwg (recovered).dwg");
}

/// FIXED (was BREAK): the "(recovered)" suffix used to STACK — the restored
/// tab's `current_path` becomes the recovered name, the next autosave round
/// snapshots it under THAT name, and the next restore appended another
/// marker ("x (recovered) (recovered).dwg"), one suffix per crash cycle,
/// unbounded. `recovered_name` now strips every trailing marker before
/// appending exactly one, so the name is idempotent across any number of
/// crash cycles.
#[test]
fn recovered_marker_never_stacks_across_crash_cycles() {
    // A single cycle: the marker is appended once, then stays at one.
    let mut name = "plan.dwg".to_string();
    for cycle in 1..=50 {
        name = recovered_name(&name);
        assert_eq!(
            name,
            "plan (recovered).dwg",
            "cycle {cycle}: the recovered name must be idempotent"
        );
        assert!(worker_accepts(&name), "{name:?} must stay parseable");
    }
    // Legacy stacked names (written by the OLD behaviour before the fix)
    // collapse back to a single marker.
    assert_eq!(
        recovered_name("plan (recovered) (recovered).dwg"),
        "plan (recovered).dwg"
    );
    assert_eq!(
        recovered_name(&format!("plan{} (recovered).dwg", " (recovered)".repeat(40))),
        "plan (recovered).dwg"
    );
    // Extensionless stacked names de-stack too, then gain the DXF default.
    assert_eq!(recovered_name("plan (recovered)"), "plan (recovered).dxf");
    // An embedded (non-suffix) marker is left alone.
    assert_eq!(
        recovered_name("(recovered) plan.dwg"),
        "(recovered) plan (recovered).dwg"
    );
    // The cycle still re-offers after a crash (no recent save of its own
    // name) — that property is unchanged, only the name growth is fixed.
    assert!(newer_than_saved(0.0, None));
}

/// Names that survive intact: no extension → DXF fallback (matching how
/// autosave serializes untitled tabs as DXF via `source_is_dxf` →
/// `dwg_source_version.is_none()`), multi-dot keeps the final extension,
/// unicode is passed through, path separators are stripped from the head so
/// the result can never escape the display-name namespace (OPFS keys are
/// FNV hashes, never the name).
#[test]
fn benign_hostile_names_round_trip_parseable() {
    for (input, expected, dxf_bytes) in [
        // Extensionless names restore as DXF — what autosave serializes
        // untitled tabs as (was ".dwg" under the old Path::extension split).
        ("Drawing1", "Drawing1 (recovered).dxf", true),
        ("a.b.dxf", "a.b (recovered).dxf", true),
        ("", " (recovered).dxf", false),
        ("plán.dwg", "plán (recovered).dwg", false),
        ("図面 1.dwg", "図面 1 (recovered).dwg", false),
    ] {
        let got = recovered_name(input);
        assert_eq!(got, expected);
        assert!(worker_accepts(&got), "{got:?} must stay parseable");
        // Real loader opens the name whenever the bytes match the extension
        // it implies (DXF fixture bytes for .dxf outcomes).
        if dxf_bytes {
            assert!(
                OpenCADStudio::io::load_bytes(&got, FIXTURE_DXF.to_vec()).is_ok(),
                "{got}"
            );
        }
    }
    // Windows path semantics: separators in the stored name never survive
    // into the recovered name as separators.
    for hostile in ["..\\evil.dxf", "../evil.dxf", "a/b\\c.dwg"] {
        let got = recovered_name(hostile);
        assert!(
            !got.contains('/') && !got.contains('\\'),
            "recovered name must not carry path separators: {got:?}"
        );
    }
    // A 4096-char name: stem grows, hash key stays 16 hex chars (see
    // web_name_hash_redteam.rs) — no length-based failure.
    let long = "x".repeat(4096 - ".dwg".len());
    let got = recovered_name(&format!("{long}.dwg"));
    assert!(worker_accepts(&got));
    assert_eq!(got.len(), long.len() + " (recovered).dwg".len());
}

// ── newer_than_saved ────────────────────────────────────────────────────────

#[test]
fn newer_than_saved_hostile_timestamps() {
    // Equal mtimes: same save round, not offered (existing behaviour).
    assert!(!newer_than_saved(1000.0, Some(1000.0)));
    // Negative mtimes compare numerically.
    assert!(newer_than_saved(-5.0, Some(-10.0)));
    assert!(!newer_than_saved(-10.0, Some(-5.0)));
    // NaN autosave mtime: `NaN > x` is false — silently NOT offered. The
    // snapshot survives (age never deletes), so worst case is a hidden
    // snapshot, not a lost one. Deterministic, no poison.
    assert!(!newer_than_saved(f64::NAN, Some(1000.0)));
    // NaN saved mtime likewise hides the offer forever.
    assert!(!newer_than_saved(1000.0, Some(f64::NAN)));
    // Extremes behave numerically: +inf / f64::MAX are "newer" than anything
    // finite; -inf is older than everything, including f64::MIN.
    assert!(newer_than_saved(f64::MAX, Some(f64::MIN)));
    assert!(newer_than_saved(f64::INFINITY, Some(f64::MAX)));
    assert!(!newer_than_saved(f64::NEG_INFINITY, Some(f64::MIN)));
}

// ── cap_evictions ───────────────────────────────────────────────────────────

#[test]
fn cap_evictions_boundaries() {
    // Empty: nothing kept, nothing evicted.
    let (kept, evicted) = cap_evictions(Vec::new(), SNAPSHOT_CAP);
    assert!(kept.is_empty() && evicted.is_empty());
    // Exactly the cap: nothing evicted.
    let entries: Vec<_> = (0..SNAPSHOT_CAP).rev().map(|i| entry(&format!("d{i}"), i as f64)).collect();
    let (kept, evicted) = cap_evictions(entries, SNAPSHOT_CAP);
    assert!(evicted.is_empty());
    assert_eq!(kept.len(), SNAPSHOT_CAP);
    assert_eq!(kept.first().unwrap().name, format!("d{}", SNAPSHOT_CAP - 1));
    // Exactly cap+1: the single oldest is evicted, survivors newest-first.
    let entries: Vec<_> = (0..=SNAPSHOT_CAP)
        .map(|i| entry(&format!("d{i}"), i as f64))
        .collect();
    let (kept, evicted) = cap_evictions(entries, SNAPSHOT_CAP);
    assert_eq!(evicted, ["d0"]);
    assert_eq!(kept.last().unwrap().name, "d1");
    assert_eq!(kept.first().unwrap().name, format!("d{SNAPSHOT_CAP}"));
    // Unsorted input still yields newest-first offer order.
    let (kept, _) = cap_evictions(
        vec![entry("old", 1.0), entry("newest", 9.0), entry("mid", 5.0)],
        SNAPSHOT_CAP,
    );
    assert_eq!(
        kept.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["newest", "mid", "old"]
    );
    // All-equal mtimes: the stable sort keeps index (push) order, `split_off`
    // takes the tail, and the final `reverse()` flips it — so among equal
    // mtimes the MOST RECENTLY INDEXED snapshot is deleted first. Arbitrary
    // (ties carry no age signal) but deterministic.
    let entries: Vec<_> = (0..SNAPSHOT_CAP + 2).map(|i| entry(&format!("e{i}"), 42.0)).collect();
    let (kept, evicted) = cap_evictions(entries, SNAPSHOT_CAP);
    assert_eq!(evicted, ["e17", "e16"]);
    assert_eq!(kept.first().unwrap().name, "e0");
    // Degenerate cap 0 evicts everything.
    let (_, evicted) = cap_evictions(vec![entry("a", 1.0), entry("b", 2.0)], 0);
    assert_eq!(evicted, ["a", "b"]); // oldest-first
}

/// NaN mtimes do NOT poison the choice into a panic or a wrong split — but
/// `total_cmp` ranks NaN ABOVE every finite mtime, so NaN entries sort as
/// "newest": they are always KEPT and cannot be evicted, while they push real
/// snapshots past the cap into eviction. `newer_than_saved(NaN, _)` then
/// refuses to offer them, so a NaN-mtime snapshot is undead: never offered,
/// never evicted, squatting one cap slot forever.
#[test]
fn cap_evictions_nan_mtimes_sort_as_newest_and_crowd_out_real_snapshots() {
    let mut entries: Vec<_> = (0..SNAPSHOT_CAP)
        .map(|i| entry(&format!("real{i}"), i as f64))
        .collect();
    entries.push(entry("nan", f64::NAN));
    let (kept, evicted) = cap_evictions(entries, SNAPSHOT_CAP);
    assert_eq!(kept.first().unwrap().name, "nan", "NaN sorts above +inf");
    assert_eq!(evicted, ["real0"], "the oldest REAL snapshot pays for it");
    assert!(!newer_than_saved(f64::NAN, Some(0.0)), "and is never offered");
    // Mixed infinities stay ordered.
    let (kept, _) = cap_evictions(
        vec![entry("neg_inf", f64::NEG_INFINITY), entry("inf", f64::INFINITY), entry("zero", 0.0)],
        SNAPSHOT_CAP,
    );
    assert_eq!(
        kept.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["inf", "zero", "neg_inf"]
    );
}
