// Faz 4c-0 preserve-verify (risk #10): an *untouched* annotative file must
// round-trip every per-object annotation-context leaf without dropping or
// mutating it. opencadcodec does not model `AcDb*ObjectContextData` — it keeps each
// leaf verbatim as `Unknown{raw_dwg_data}` and re-emits it on same-version save,
// while side-mapping the leaf's `340` annotation-scale handle into
// `context_scales`. This certifies that passthrough is intact at the current
// opencadcodec HEAD before any encoder work touches the save path.
//
// Uses the golden reference `~/Downloads/0718-mbmdmc.dwg` (AC1032/R2018). The
// test skips (does not fail) when that file is absent so it never breaks CI.

use OpenCADStudio::io;
use codec::objects::ObjectType;

const GOLDEN: &str = "/home/hakanseven/Downloads/0718-mbmdmc.dwg";

/// Raw bytes of every annotation-context leaf, sorted so the comparison is
/// independent of handle ordering/renumbering across the round-trip.
fn leaf_blobs(doc: &codec::CadDocument) -> Vec<Vec<u8>> {
    let mut blobs: Vec<Vec<u8>> = doc
        .context_scales
        .keys()
        .filter_map(|h| match doc.objects.get(h) {
            Some(ObjectType::Unknown { raw_dwg_data: Some(raw), .. }) => Some(raw.clone()),
            _ => None,
        })
        .collect();
    blobs.sort();
    blobs
}

#[test]
fn annotative_leaf_contexts_survive_dwg_roundtrip() {
    let Ok(bytes) = std::fs::read(GOLDEN) else {
        eprintln!("SKIP: golden file {GOLDEN} not present");
        return;
    };

    let doc = io::load_bytes("0718-mbmdmc.dwg", bytes).expect("load golden");
    let n0 = doc.context_scales.len();
    let leaves0 = leaf_blobs(&doc);
    eprintln!(
        "loaded: version={:?} context_scales={} preserved_leaf_blobs={}",
        doc.version,
        n0,
        leaves0.len()
    );
    assert!(n0 > 0, "golden file must carry per-object annotation contexts");

    // Same-version DWG round-trip (AC1032 -> AC1032).
    let out = io::save_to_bytes(&doc, "dwg", doc.version).expect("save dwg bytes");
    let doc2 = io::load_bytes("roundtrip.dwg", out).expect("reload dwg bytes");
    let n1 = doc2.context_scales.len();
    let leaves1 = leaf_blobs(&doc2);
    eprintln!("reloaded: context_scales={} preserved_leaf_blobs={}", n1, leaves1.len());

    assert_eq!(n0, n1, "annotation-context leaf COUNT changed across round-trip");
    assert_eq!(
        leaves0.len(),
        leaves1.len(),
        "preserved leaf raw-byte-blob count changed across round-trip"
    );
    assert_eq!(
        leaves0, leaves1,
        "annotation-context leaf raw bytes are NOT byte-identical after round-trip"
    );
}

/// Diagnostic: tally the golden file's context leaves by their DXF class name so
/// we know which leaf type to encode + byte-diff first (4c-1). Not an assertion.
#[test]
fn annotative_leaf_type_breakdown() {
    let Ok(bytes) = std::fs::read(GOLDEN) else {
        eprintln!("SKIP: golden file {GOLDEN} not present");
        return;
    };
    let doc = io::load_bytes("0718-mbmdmc.dwg", bytes).expect("load golden");

    let mut tally: std::collections::BTreeMap<String, usize> = Default::default();
    let mut sizes: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    for h in doc.context_scales.keys() {
        if let Some(ObjectType::Unknown { type_name, raw_dwg_data, .. }) = doc.objects.get(h) {
            // type_name is "DWG_OBJ_<type_code>"; resolve the code to a class name.
            let name = type_name
                .rsplit('_')
                .next()
                .and_then(|n| n.parse::<i16>().ok())
                .and_then(|code| doc.classes.iter().find(|c| c.class_number == code))
                .map(|c| c.dxf_name.clone())
                .unwrap_or_else(|| type_name.clone());
            *tally.entry(name.clone()).or_default() += 1;
            let len = raw_dwg_data.as_ref().map(|r| r.len()).unwrap_or(0);
            let e = sizes.entry(name).or_insert((usize::MAX, 0));
            e.0 = e.0.min(len);
            e.1 = e.1.max(len);
        }
    }
    eprintln!("=== context-leaf type breakdown ({} total) ===", doc.context_scales.len());
    for (name, n) in &tally {
        let (lo, hi) = sizes[name];
        eprintln!("  {n:>4}  {name}   raw_bytes {lo}..{hi}");
    }
}

/// Dump one real BLKREF context leaf: its raw data bytes, handle bits, owner,
/// and its 340 scale target — to reverse the exact bit layout for the encoder.
#[test]
fn dump_one_blkref_leaf() {
    let Ok(bytes) = std::fs::read(GOLDEN) else {
        eprintln!("SKIP: golden file {GOLDEN} not present");
        return;
    };
    let doc = io::load_bytes("0718-mbmdmc.dwg", bytes).expect("load golden");

    // Find the BLKREF class number.
    let blkref_code = doc
        .classes
        .iter()
        .find(|c| c.dxf_name == "ACDB_BLKREFOBJECTCONTEXTDATA_CLASS")
        .map(|c| c.class_number);
    eprintln!("BLKREF class_number = {blkref_code:?}");

    let mut shown = 0;
    for (h, scale_h) in &doc.context_scales {
        if shown >= 2 {
            break;
        }
        if let Some(ObjectType::Unknown {
            type_name,
            owner,
            raw_dwg_data: Some(raw),
            raw_dwg_handle_bits,
            raw_dwg_version,
            ..
        }) = doc.objects.get(h)
        {
            let code: Option<i16> = type_name.rsplit('_').next().and_then(|n| n.parse().ok());
            if code != blkref_code {
                continue;
            }
            shown += 1;
            eprintln!("--- BLKREF leaf handle={h:?} owner={owner:?} scale_target={scale_h:?} ver={raw_dwg_version:?}");
            eprintln!("    data ({} B): {}", raw.len(), hex(raw));
            eprintln!("    handle_bits: {:?}", raw_dwg_handle_bits);
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}
