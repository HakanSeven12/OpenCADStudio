use super::{prepare_document_for_save, prepare_save_batch};
use crate::perf::audit;
use document::{
    entities::{Dimension, DimensionLinear},
    types::Vector3,
    Document, EntityType,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn drawing(tab: usize, dimensions: usize) -> Document {
    let mut doc = Document::new();
    crate::io::linetypes::populate_document(&mut doc);
    for i in 0..dimensions {
        let x = (i % 25) as f64 * 30.0;
        let y = (i / 25) as f64 * 20.0 + tab as f64;
        let mut dim = DimensionLinear::new(Vector3::new(x, y, 0.0), Vector3::new(x + 10.0, y, 0.0));
        dim.definition_point = Vector3::new(x + 5.0, y + 7.0, 0.0);
        dim.base.common.handle = doc.allocate_handle();
        doc.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(dim))))
            .unwrap();
    }
    doc
}

#[derive(Serialize, Deserialize)]
struct Sample {
    batch_us: f64,
    first_write_ready_us: f64,
    largest_preparation_chunk_us: f64,
    output_bytes: usize,
    entities: usize,
    hashes: Vec<[u8; 32]>,
}

#[test]
#[ignore = "child of release performance audit"]
fn performance_audit_save_child() {
    let Ok(case) = std::env::var("OCS_AUDIT_SAVE_CASE") else {
        return;
    };
    assert!(!cfg!(debug_assertions));
    crate::perf::set_ui_enabled(false);
    let parts: Vec<_> = case.split(':').collect();
    let (flow, strategy) = (parts[0], parts[1]);
    let dimensions = parts[2].parse::<usize>().unwrap();
    let warm = drawing(0, 2);
    let prepared = prepare_document_for_save(warm.clone(), warm.version);
    black_box(Sha256::digest(
        ocs_save::serialize_document(&prepared, "dxf").unwrap(),
    ));
    drop((warm, prepared));
    let live: Vec<_> = (0..6).map(|i| drawing(i, dimensions)).collect();
    let pending: Vec<_> = live
        .iter()
        .enumerate()
        .map(|(i, d)| (i, d.clone(), d.version))
        .collect();
    let mut retained = Vec::with_capacity(6);
    let mut prep = Vec::with_capacity(6);
    let mut hashes = Vec::with_capacity(6);
    let mut bytes_total = 0;
    let mut entities = 0;
    let mut first = 0.0;
    let mut validation = Duration::ZERO;
    let batch = Instant::now();
    let mut write = |(index, doc): (usize, Document)| {
        if index == 0 {
            first = batch.elapsed().as_secs_f64() * 1e6;
        }
        let bytes = ocs_save::serialize_document(&doc, "dxf").unwrap();
        let check = Instant::now();
        hashes.push(Sha256::digest(&bytes).into());
        bytes_total += bytes.len();
        entities += doc.entities().count();
        validation += check.elapsed();
        black_box((&doc, &bytes));
        if flow == "saveall" {
            retained.push(bytes);
        }
    };
    let chunk;
    if strategy == "eager" {
        let start = Instant::now();
        let all: Vec<_> = pending
            .into_iter()
            .map(|(i, d, v)| {
                let start = Instant::now();
                let doc = prepare_document_for_save(d, v);
                prep.push(start.elapsed().as_secs_f64() * 1e6);
                (i, doc)
            })
            .collect();
        chunk = start.elapsed().as_secs_f64() * 1e6;
        for doc in all {
            write(doc);
        }
    } else {
        let mut iter = prepare_save_batch(pending);
        loop {
            let start = Instant::now();
            let doc = iter.next();
            let elapsed = start.elapsed().as_secs_f64() * 1e6;
            let Some(doc) = doc else {
                break;
            };
            prep.push(elapsed);
            write(doc);
        }
        chunk = prep.iter().copied().fold(0.0, f64::max);
    }
    let elapsed = batch.elapsed().saturating_sub(validation).as_secs_f64() * 1e6;
    black_box((&live, &retained));
    assert_eq!(hashes.len(), 6);
    assert!(entities > dimensions * 6);
    eprintln!(
        "AUDIT_SAMPLE {}",
        serde_json::to_string(&Sample {
            batch_us: elapsed,
            first_write_ready_us: first,
            largest_preparation_chunk_us: chunk,
            output_bytes: bytes_total,
            entities,
            hashes,
        })
        .unwrap()
    );
}

fn child(flow: &str, strategy: &str, dimensions: usize) -> Sample {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "io::performance_audit::performance_audit_save_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_remove("PERF")
        .env(
            "OCS_AUDIT_SAVE_CASE",
            format!("{flow}:{strategy}:{dimensions}"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stderr).unwrap();
    serde_json::from_str(
        text.lines()
            .find_map(|line| line.strip_prefix("AUDIT_SAMPLE "))
            .expect("child result"),
    )
    .unwrap()
}

#[test]
#[ignore = "release performance audit; 30 fresh paired processes per case"]
fn performance_audit_save() {
    assert!(!cfg!(debug_assertions), "use --release");
    let mut results = Vec::new();
    for dimensions in [250, 1000] {
        for flow in ["autosave", "saveall"] {
            let mut before = Vec::new();
            let mut after = Vec::new();
            for i in 0..30 {
                let (old, new) = if i % 2 == 0 {
                    (
                        child(flow, "eager", dimensions),
                        child(flow, "lazy", dimensions),
                    )
                } else {
                    let new = child(flow, "lazy", dimensions);
                    (child(flow, "eager", dimensions), new)
                };
                assert_eq!(old.hashes, new.hashes);
                assert_eq!(old.output_bytes, new.output_bytes);
                assert_eq!(old.entities, new.entities);
                if let Some(first) = before.first() {
                    let first: &Sample = first;
                    assert_eq!(first.hashes, old.hashes, "workload changed across samples");
                }
                before.push(old);
                after.push(new);
            }
            let batch = audit::compare(
                before.iter().map(|s| s.batch_us).collect(),
                after.iter().map(|s| s.batch_us).collect(),
            );
            let first = audit::compare(
                before.iter().map(|s| s.first_write_ready_us).collect(),
                after.iter().map(|s| s.first_write_ready_us).collect(),
            );
            let chunk = audit::compare(
                before
                    .iter()
                    .map(|s| s.largest_preparation_chunk_us)
                    .collect(),
                after
                    .iter()
                    .map(|s| s.largest_preparation_chunk_us)
                    .collect(),
            );
            eprintln!(
                "SAVE {flow} 6x{dimensions}: {:.3} -> {:.3} ms median; {:.3} -> {:.3} ms p95",
                batch.before.median_us / 1000.0,
                batch.after.median_us / 1000.0,
                batch.before.p95_us / 1000.0,
                batch.after.p95_us / 1000.0
            );
            results.push(serde_json::json!({"flow":flow,"tabs":6,"dimensions_per_tab":dimensions,
                "output_bytes":before[0].output_bytes,"hashes":before[0].hashes,"batch":batch,"first_write_ready":first,"preparation_chunk":chunk}));
        }
    }
    audit::write_report(
        "save",
        serde_json::json!({
            "scope":"native shared preparation/writer pipeline; release, no allocation counting; setup and output-validation hashing excluded; fresh process per arm; exact DXF bytes verified; excludes browser JS/worker transfer, OPFS and frame latency",
            "cases":results,
        }),
    );
}
