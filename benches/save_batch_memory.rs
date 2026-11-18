//! Native allocation probe for the shared save-preparation/writer path used
//! by browser AUTOSAVE and SAVEALL. Each strategy runs in a fresh process.
//! This measures neither browser/JS worker memory nor native concurrent saves.

#[path = "support/counting_allocator.rs"]
mod allocation;

use document::entities::{Dimension, DimensionLinear};
use document::types::Vector3;
use document::{Document, EntityType};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::hint::black_box;
use OpenCADStudio::io::{prepare_document_for_save, prepare_save_batch};

#[global_allocator]
static ALLOCATOR: allocation::CountingAllocator = allocation::CountingAllocator;

const TABS: usize = 6;
const DIMENSIONS: usize = 250;

fn drawing(tab: usize, dimensions: usize) -> Document {
    let mut doc = Document::new();
    OpenCADStudio::io::linetypes::populate_document(&mut doc);
    for i in 0..dimensions {
        let x = (i % 25) as f64 * 30.0;
        let y = (i / 25) as f64 * 20.0 + tab as f64;
        let mut dimension =
            DimensionLinear::new(Vector3::new(x, y, 0.0), Vector3::new(x + 10.0, y, 0.0));
        dimension.definition_point = Vector3::new(x + 5.0, y + 7.0, 0.0);
        dimension.base.common.handle = doc.allocate_handle();
        doc.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(
            dimension,
        ))))
        .unwrap();
    }
    doc
}

struct DigestWriter(Sha256);
impl std::io::Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Output {
    tab: usize,
    entities: usize,
    blocks: usize,
    bytes: usize,
    sha256: [u8; 32],
}

#[derive(Serialize, Deserialize)]
struct Report {
    schema: u32,
    strategy: String,
    flow: String,
    revision: String,
    os: String,
    arch: String,
    profile: String,
    tabs: usize,
    dimensions_per_tab: usize,
    fixture_sha256: String,
    baseline_live_bytes: usize,
    peak_live_bytes: usize,
    peak_above_baseline_bytes: usize,
    live_after_writes_bytes: usize,
    outputs: Vec<Output>,
    scope: String,
}

fn measure(strategy: &str, flow: &str) -> Report {
    allocation::self_test();
    std::env::remove_var("PERF");
    // Prime fonts, worker pools and serialization before either baseline.
    let warmup = drawing(0, 2);
    let prepared = prepare_document_for_save(warmup.clone(), warmup.version);
    black_box(ocs_save::serialize_document(&prepared, "dxf").unwrap());
    drop((warmup, prepared));

    // Live documents stay alive throughout: capture does not close the tabs.
    let live_documents: Vec<_> = (0..TABS).map(|i| drawing(i, DIMENSIONS)).collect();
    let mut fixture = DigestWriter(Sha256::new());
    fixture
        .0
        .update(b"save-batch-v1;6-tabs;250-linear-dimensions;dxf;fresh-dimension-blocks");
    for doc in &live_documents {
        // Header timestamps and unrelated per-process metadata are excluded.
        for entity in doc.entities() {
            serde_json::to_writer(&mut fixture, entity).unwrap();
        }
    }
    let fixture_sha256 = format!("{:x}", fixture.0.finalize());
    let snapshots: Vec<_> = live_documents
        .iter()
        .enumerate()
        .map(|(i, doc)| (i, doc.clone(), doc.version))
        .collect();
    let mut outputs = Vec::with_capacity(TABS);
    let mut retained_outputs = Vec::with_capacity(TABS);
    let baseline = allocation::reset_peak();
    let mut save = |(tab, prepared): (usize, Document)| {
        let bytes = ocs_save::serialize_document(&prepared, "dxf").expect("DXF serialization");
        outputs.push(Output {
            tab,
            entities: prepared.entities().count(),
            blocks: prepared.block_records.len(),
            bytes: bytes.len(),
            sha256: Sha256::digest(&bytes).into(),
        });
        black_box((&prepared, &bytes));
        if flow == "saveall" {
            retained_outputs.push(bytes);
        }
        // AUTOSAVE drops written bytes; SAVEALL keeps them for its download
        // completion. Both drop prepared geometry before the next document.
    };
    match strategy {
        "eager" => {
            let prepared: Vec<_> = snapshots
                .into_iter()
                .map(|(tab, doc, version)| (tab, prepare_document_for_save(doc, version)))
                .collect();
            for document in prepared {
                save(document);
            }
        }
        "lazy" => {
            for document in prepare_save_batch(snapshots) {
                save(document);
            }
        }
        _ => panic!("unknown strategy"),
    }
    black_box(&retained_outputs);
    let peak_live_bytes = allocation::peak();
    let live_after_writes_bytes = allocation::live();
    assert_eq!(outputs.len(), TABS);
    assert!(outputs
        .iter()
        .all(|out| out.entities > DIMENSIONS && out.blocks >= DIMENSIONS));
    black_box(&live_documents);
    Report {
        schema: 1, strategy: strategy.into(), flow: flow.into(), revision: env!("OCS_GIT_REV").into(),
        os: std::env::consts::OS.into(), arch: std::env::consts::ARCH.into(),
        profile: if cfg!(debug_assertions) { "debug" } else { "release" }.into(),
        tabs: TABS, dimensions_per_tab: DIMENSIONS, fixture_sha256,
        baseline_live_bytes: baseline, peak_live_bytes,
        peak_above_baseline_bytes: peak_live_bytes.saturating_sub(baseline),
        live_after_writes_bytes, outputs,
        scope: "Rust System requested allocation bytes for captured snapshots, preparation and serial DXF writing; fixture/warmup excluded from peak delta; live documents retained; excludes allocator overhead, GUI/GPU, JS/worker transfers and child processes; timings not measured".into(),
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--self-test") {
        allocation::self_test();
        println!("allocator accounting passed");
        return;
    }
    let flow = args.iter().position(|arg| arg == "--flow").map(|index| {
        args.get(index + 1)
            .expect("--flow needs autosave or saveall")
            .as_str()
    });
    if let Some(flow) = flow {
        assert!(["autosave", "saveall"].contains(&flow));
    }
    if let Some(index) = args.iter().position(|arg| arg == "--strategy") {
        let strategy = args.get(index + 1).expect("--strategy needs eager or lazy");
        assert!(["eager", "lazy"].contains(&strategy.as_str()));
        println!(
            "{}",
            serde_json::to_string_pretty(&measure(strategy, flow.unwrap_or("autosave"))).unwrap()
        );
        return;
    }
    let output = args.iter().position(|arg| arg == "--output").map(|index| {
        std::path::PathBuf::from(args.get(index + 1).expect("--output needs a new path"))
    });
    if let Some(path) = &output {
        assert!(!path.exists(), "output must be new");
    }
    let exe = std::env::current_exe().unwrap();
    let flows = flow.map_or_else(|| vec!["autosave", "saveall"], |flow| vec![flow]);
    let mut reports = Vec::new();
    for flow in flows {
        let mut pair = Vec::new();
        for strategy in ["eager", "lazy"] {
            let child = std::process::Command::new(&exe)
                .args(["--strategy", strategy, "--flow", flow])
                .output()
                .unwrap();
            assert!(
                child.status.success(),
                "{flow}/{strategy} failed: {}",
                String::from_utf8_lossy(&child.stderr)
            );
            pair.push(
                serde_json::from_slice::<Report>(&child.stdout).expect("strategy JSON report"),
            );
        }
        assert_eq!(
            pair[0].fixture_sha256, pair[1].fixture_sha256,
            "fixture changed"
        );
        assert_eq!(pair[0].outputs, pair[1].outputs, "saved output changed");
        let improved = pair[1].peak_above_baseline_bytes < pair[0].peak_above_baseline_bytes;
        // Preserve failing reports for inspection before the check below.
        reports.extend(pair);
        if !improved {
            eprintln!("{flow}: lazy preparation did not reduce peak allocation bytes");
        }
    }
    let json = serde_json::to_string_pretty(&reports).unwrap();
    if let Some(path) = output {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();
    }
    println!("{json}");
    assert!(
        reports
            .chunks_exact(2)
            .all(|pair| pair[1].peak_above_baseline_bytes < pair[0].peak_above_baseline_bytes),
        "lazy preparation must reduce each flow's peak on this fixture"
    );
}
