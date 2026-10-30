//! OpenCADStudio UI label translation benchmark.
//!
//! Every `t!` call resolves a source string to a catalog id (memoized since
//! `14ca1b07a`) and then renders that id through Fluent, which walks the
//! language bundles and allocates the translated string. A view rebuild
//! translates hundreds of labels, so the per-call cost is paid hundreds of
//! times whenever the UI rebuilds while dragging or navigating.
//!
//! Fixture: 64 distinct catalog labels (each a real entry from
//! `locale_catalog.rs`), laid out across a 512-label rebuild — the shape of a
//! ribbon/chrome pass. A second phase covers `tr!` ids (semantic Fluent ids
//! rather than source strings) at 64 lookups per rebuild.
//!
//! Sink: a digest of every translated label is taken before timing and must
//! match after all runs, so a memo cannot change what the UI renders.
//!
//! Deterministic: fixed labels, English fallback, no randomness.
//! Run: `cargo bench --bench i18n_label_render`

use std::hint::black_box;
use std::time::Instant;

use OpenCADStudio::i18n;

const LABELS: [&str; 64] = [
    "Import Styles",
    "Select Drawing to Import Styles From",
    "Select the styles to import",
    "No styles found in this drawing.",
    "Existing",
    "The Lookup table is invalid.",
    "Usage: DCDISPLAY [Show|Hide]",
    "Select dimensions to disassociate:",
    "Clear overrides",
    "Open in block editor after creating",
    "No objects are selected for the block.",
    "Create empty block",
    "Block Attribute Manager",
    "Modes",
    "Columns:",
    "Apply changes to existing references",
    "Emphasize duplicate tags",
    "Sync",
    "Specify opposite corner:",
    "Create Drawing File",
    "Add to Working Set",
    "Remove from Working Set",
    "Select reference:",
    "Reference not found.",
    "Object is not a valid reference",
    "1 entity added",
    "Select nesting level [Ok/Next] <Next>:",
    "Select nested objects:",
    "0 items selected",
    "1 item selected",
    "Enter an option [Add/Remove] <Add>:",
    "1 xref instance updated",
    "1 block instance updated",
    "Ok",
    "Nested",
    "Operational Symbols",
    "Decimal Separator",
    "Open 3Dconnexion settings...",
    "Open 3Dconnexion settings…",
    "Follow context",
    "3DxWare unavailable",
    "Downloading...",
    "Download available",
    "Skip",
    "Missing fonts",
    "Coincident",
    "Colinear",
    "Concentric",
    "Symmetric",
    "Equal",
    "Fix",
    "GPU (backend)",
    "Number of aliases",
    "Origin planes",
    "No sketch open",
    "No solids yet",
    "This session",
    "Driver",
    "Backend",
    "Selected by",
    "Next launch",
    "Graphics backend",
    "Paused",
    "Enable SpaceMouse",
];

const LABELS_PER_REBUILD: usize = 512;
const TR_LOOKUPS_PER_REBUILD: usize = 64;
const REBUILDS: usize = 200;
const RUNS: usize = 3;
const WARMUP: usize = 3;

fn digest(bytes: impl Iterator<Item = u64>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= byte;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn t_rebuild() -> u64 {
    let mut labels = Vec::with_capacity(LABELS_PER_REBUILD);
    for slot in 0..LABELS_PER_REBUILD {
        labels.push(i18n::translate(LABELS[slot % LABELS.len()]));
    }
    digest(labels.iter().flat_map(|label| label.as_bytes().iter().copied().map(u64::from)))
}

fn tr_rebuild() -> u64 {
    let mut labels = Vec::with_capacity(TR_LOOKUPS_PER_REBUILD);
    for _ in 0..TR_LOOKUPS_PER_REBUILD {
        labels.push(OpenCADStudio::tr!("ribbon-tab", "draw"));
    }
    digest(labels.iter().flat_map(|label| label.as_bytes().iter().copied().map(u64::from)))
}

fn main() {
    let t_digest = t_rebuild();
    let tr_digest = tr_rebuild();
    assert!(t_digest != 0 && tr_digest != 0);
    for _ in 0..WARMUP {
        black_box(t_rebuild());
        black_box(tr_rebuild());
    }

    let mut best_t = f64::MAX;
    let mut best_tr = f64::MAX;
    for run in 0..RUNS {
        let started = Instant::now();
        for _ in 0..REBUILDS {
            black_box(t_rebuild());
        }
        let t_per_rebuild = started.elapsed().as_secs_f64() * 1e6 / REBUILDS as f64;

        let started = Instant::now();
        for _ in 0..REBUILDS {
            black_box(tr_rebuild());
        }
        let tr_per_rebuild = started.elapsed().as_secs_f64() * 1e6 / REBUILDS as f64;

        best_t = best_t.min(t_per_rebuild);
        best_tr = best_tr.min(tr_per_rebuild);
        println!(
            "i18n_label_render: run {run} t! {t_per_rebuild:.3} us/rebuild, tr! {tr_per_rebuild:.3} us/rebuild"
        );
    }

    assert_eq!(t_rebuild(), t_digest, "t! labels must not change");
    assert_eq!(tr_rebuild(), tr_digest, "tr! labels must not change");
    println!(
        "i18n_label_render: {LABELS_PER_REBUILD} t! labels + {TR_LOOKUPS_PER_REBUILD} tr! ids per rebuild; best {best_t:.3} / {best_tr:.3} us/rebuild"
    );
}
