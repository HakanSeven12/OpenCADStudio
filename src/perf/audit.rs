//! Opt-in performance audit infrastructure. No counting allocator or production
//! application changes. Durations use microseconds; reports retain raw samples.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
pub(crate) struct Distribution {
    pub samples: Vec<f64>,
    pub median_us: f64,
    pub p95_us: f64,
    pub min_us: f64,
    pub max_us: f64,
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    }
}

impl Distribution {
    fn new(samples: Vec<f64>) -> Self {
        assert!(!samples.is_empty() && samples.iter().all(|v| v.is_finite() && *v >= 0.0));
        let mut sorted = samples.clone();
        sorted.sort_by(f64::total_cmp);
        Self {
            median_us: median(&samples),
            p95_us: sorted
                [((sorted.len() as f64 * 0.95).ceil() as usize - 1).min(sorted.len() - 1)],
            min_us: sorted[0],
            max_us: *sorted.last().unwrap(),
            samples,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct Comparison {
    pub before: Distribution,
    pub after: Distribution,
    pub median_change_percent: f64,
    pub paired_median_delta_us: f64,
    pub paired_delta_95ci_us: [f64; 2],
}

pub(crate) fn compare(before: Vec<f64>, after: Vec<f64>) -> Comparison {
    assert_eq!(before.len(), after.len());
    let deltas: Vec<_> = before.iter().zip(&after).map(|(a, b)| b - a).collect();
    let mut rng = 0x53a7_ea19_31cb_942du64;
    let mut resampled = vec![0.0; deltas.len()];
    let mut medians = Vec::with_capacity(4000);
    for _ in 0..4000 {
        for value in &mut resampled {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            *value = deltas[rng as usize % deltas.len()];
        }
        medians.push(median(&resampled));
    }
    medians.sort_by(f64::total_cmp);
    let before = Distribution::new(before);
    let after = Distribution::new(after);
    Comparison {
        median_change_percent: (after.median_us / before.median_us - 1.0) * 100.0,
        before,
        after,
        paired_median_delta_us: median(&deltas),
        paired_delta_95ci_us: [medians[100], medians[3899]],
    }
}

/// Alternate order to reduce drift; each closure owns its setup/timed boundary.
pub(crate) fn paired(
    n: usize,
    mut before: impl FnMut() -> f64,
    mut after: impl FnMut() -> f64,
) -> Comparison {
    let mut old = Vec::with_capacity(n);
    let mut new = Vec::with_capacity(n);
    for i in 0..n {
        if i % 2 == 0 {
            old.push(before());
            new.push(after());
        } else {
            let value = after();
            old.push(before());
            new.push(value);
        }
    }
    compare(old, new)
}

pub(crate) fn write_report(name: &str, value: impl Serialize) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/performance-audit");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{name}.json"));
    let report = serde_json::json!({
        "schema": 1, "revision": env!("OCS_GIT_REV"), "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "allocator": if cfg!(target_os = "windows") { "mimalloc (same as application)" } else { "system (same as application)" },
        "logical_cpus": std::thread::available_parallelism().map_or(1, usize::from),
        "counting_allocator": false,
        "results": value,
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("AUDIT_REPORT {}", path.display());
    path
}

#[test]
#[ignore = "release performance audit statistics sanity check"]
fn performance_audit_statistics() {
    let comparison = compare(vec![10.0; 100], vec![12.0; 100]);
    assert_eq!(comparison.before.median_us, 10.0);
    assert_eq!(comparison.after.p95_us, 12.0);
    assert_eq!(comparison.paired_delta_95ci_us, [2.0, 2.0]);
    assert!((comparison.median_change_percent - 20.0).abs() < 1e-9);
    assert_eq!(
        Distribution::new((1..=100).map(f64::from).collect()).p95_us,
        95.0
    );
}
