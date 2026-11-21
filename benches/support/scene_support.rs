//! SCN-012 shared bench support: environment capture, hand-rolled percentile
//! math, and the JSON RUN RECORD printer.
//!
//! NEW file owned by SCN-012, used ONLY by `benches/scene_scaling.rs` and
//! `benches/widget_state_scaling.rs` (included via `#[path = ...] mod`). The
//! pre-existing `benches/support/harness.rs` was left untouched: it is wired
//! into the `performance_benchmarks` suite (baseline comparison + table
//! printer) and does not model the per-scenario honesty labels this task
//! needs, so sharing did not demand reusing it.
//!
//! Conventions (binding, `benches/README.md`): these benches are placeholder
//! binaries — `cargo bench --bench <name>` runs them, regular `cargo test`
//! runs never do. Each bench prints human-readable per-sample lines, then ONE
//! pretty-printed JSON record (run-record format `scn-012-run-record/v1`)
//! containing the env block, per-scenario params/statistics, payload byte
//! sizes, counter entries, and the explicit `changed < total` acceptance
//! numbers, and then exits 0.
//!
//! Honesty rule: every scenario carries a `surface` label naming exactly
//! which crate-visibility surface it measured; anything `pub(crate)` on the
//! host side is emitted as a `Counter` with `scope: "deferred"` plus a note
//! pointing at the in-source unit tests that hold the evidence today.
//! No number overclaims what it measured.

#![allow(dead_code)] // each bench target uses its own subset of this module

use serde::Serialize;
use std::env;
use std::hint::black_box;
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Identifies the exact record shape read by the SCN-012 coordinator tooling.
pub const RUN_RECORD_FORMAT: &str = "scn-012-run-record/v1";

// ---------------------------------------------------------------------------
// Wall-clock sampling + percentiles (hand-rolled, std only)
// ---------------------------------------------------------------------------

/// A cumulative distribution read over per-sample wall times in ns.
///
/// Percentiles are NEAREST-RANK over the ascending-sorted samples:
/// `p(rank) = sorted[ceil(rank * n) - 1]` (clamped to the last index), the
/// usual definition for this kind of report. min/max are the sorted ends.
#[derive(Debug, Clone, Serialize)]
pub struct WallPercentiles {
    /// Number of timed samples the percentiles were computed over.
    pub n: usize,
    pub p50_ns: f64,
    pub p95_ns: f64,
    pub p99_ns: f64,
    pub min_ns: f64,
    pub max_ns: f64,
}

impl WallPercentiles {
    /// Sample N wall times and reduce them. `warmup` untimed runs settle
    /// caches/allocator state before the first sample.
    pub fn sample(n: usize, warmup: usize, mut op: impl FnMut()) -> Self {
        for _ in 0..warmup {
            op();
        }
        let mut samples: Vec<f64> = Vec::with_capacity(n);
        for _ in 0..n {
            let start = Instant::now();
            op();
            samples.push(start.elapsed().as_nanos() as f64);
        }
        Self::from_samples(&samples)
    }

    /// Reduce already-collected ns samples (nearest-rank percentiles).
    pub fn from_samples(samples: &[f64]) -> Self {
        assert!(
            !samples.is_empty(),
            "percentiles require at least one sample"
        );
        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let n = sorted.len();
        Self {
            n,
            p50_ns: nearest_rank(&sorted, 0.50),
            p95_ns: nearest_rank(&sorted, 0.95),
            p99_ns: nearest_rank(&sorted, 0.99),
            min_ns: sorted[0],
            max_ns: sorted[n - 1],
        }
    }
}

/// Nearest-rank percentile of an ascending-sorted slice: sorted index
/// `ceil(rank * n) - 1`, clamped. `rank` in `(0, 1]`.
fn nearest_rank(sorted: &[f64], rank: f64) -> f64 {
    let n = sorted.len();
    let idx = ((rank * n as f64).ceil() as usize).max(1).min(n) - 1;
    sorted[idx]
}

// ---------------------------------------------------------------------------
// Environment block (host, os, rustc, git commit — captured at bench time)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct EnvBlock {
    pub bench: String,
    pub host: String,
    pub os: String,
    pub arch: String,
    pub profile: String,
    pub kernel: String,
    pub rustc: String,
    pub git_commit: String,
    pub git_dirty: bool,
    pub timestamp_unix: u64,
    pub pid: u32,
}

fn subprocess_trimmed(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_owned())
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| env::var("HOSTNAME").ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "<unknown>".to_owned())
}

fn kernel_release() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| "<unknown>".to_owned())
}

/// Capture the run's identity at bench time. `git rev-parse HEAD` runs
/// against the bench binary's current working directory (the repo/package
/// root when launched via `cargo bench`), which resolves inside a worktree.
pub fn env_block(bench: &str) -> EnvBlock {
    let rustc = subprocess_trimmed(env::var("RUSTC").unwrap_or_else(|_| "rustc".into()).as_str(), &[
        "--version",
    ])
    .unwrap_or_else(|| "<rustc-unavailable>".to_owned());
    let git_commit = subprocess_trimmed("git", &["rev-parse", "HEAD"])
        .unwrap_or_else(|| "<git-unavailable>".to_owned());
    let git_dirty = subprocess_trimmed("git", &["status", "--porcelain"])
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let timestamp_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    EnvBlock {
        bench: bench.to_owned(),
        host: hostname(),
        os: env::consts::OS.to_owned(),
        arch: env::consts::ARCH.to_owned(),
        profile: if cfg!(debug_assertions) {
            "debug".to_owned()
        } else {
            "release".to_owned()
        },
        kernel: kernel_release(),
        rustc,
        git_commit,
        git_dirty,
        timestamp_unix,
        pid: std::process::id(),
    }
}

// ---------------------------------------------------------------------------
// Run record pieces
// ---------------------------------------------------------------------------

/// One `name = value` scenario parameter. Values are JSON values (numbers,
/// strings, bools) so the record stays schema-flexible per scenario.
#[derive(Debug, Serialize)]
pub struct Param {
    pub name: String,
    pub value: serde_json::Value,
}

/// A scenario parameter built from any JSON-convertible value.
pub fn param(name: &'static str, value: impl Into<serde_json::Value>) -> Param {
    Param {
        name: name.to_owned(),
        value: value.into(),
    }
}

/// One counter-style fact.
///
/// - `scope: "measured"` — directly read from the API surface the bench
///   exercised (e.g. report facts returned by `apply_patch_report`).
/// - `scope: "bench-emulated"` — the bench computed the quantity from the
///   measured surface (e.g. touched-region element walks standing in for the
///   host store's re-projection granularity); the note names the emulation.
/// - `scope: "deferred"` — the authoritative counter lives behind
///   `pub(crate)` (host-only); value is `None` today, note carries where its
///   unit-test evidence lives and when the wall-clock bench is planned.
#[derive(Debug, Serialize)]
pub struct Counter {
    pub name: String,
    pub scope: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A secondary timed measurement of one scenario (extra op / anti-pattern /
/// validation gate), each with its own percentiles.
#[derive(Debug, Serialize)]
pub struct ExtraMetric {
    pub label: String,
    pub samples_n: usize,
    pub wall_ns: WallPercentiles,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One benchmark scenario: what it measured, its params, its wall-clock
/// percentiles, payload bytes, counters, and the acceptance numbers.
#[derive(Debug, Serialize)]
pub struct ScenarioRecord {
    pub scenario: String,
    /// Exactly which surface produced the numbers (crate visibility honesty
    /// label). Example: "pub ocs_plugin_api::scene (procs: apply_patch_report,
    /// scene_summary) — host store internals pub(crate), see counters/notes".
    pub surface: String,
    /// What this scenario deliberately did NOT measure and why (the
    /// measured-vs-deferred split, recorded per scenario).
    pub deferred: Vec<String>,
    pub params: Vec<Param>,
    pub samples_n: usize,
    pub wall_ns: WallPercentiles,
    /// Secondary timed measurements of the same scenario (a bounded read,
    /// an O(touched) walk, an anti-pattern sample), each with its own
    /// percentiles.
    pub extra_metrics: Vec<ExtraMetric>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_bytes_note: Option<String>,
    pub counters: Vec<Counter>,
    /// Elements (or widgets/items) the patch changed, the milestone-2
    /// acceptance numerator. `None` for scenarios without a patch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_elements: Option<u64>,
    /// Total scene size (the denominator); `None` when not applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_elements: Option<u64>,
    /// The explicit acceptance fact `changed_elements < total_elements`.
    /// `None` when the scenario has no changed/total pair to compare.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_less_than_total: Option<bool>,
    /// Correctness sink: what the samples had to agree on for the timing to
    /// mean anything (printed so the number cannot be read without the
    /// assertion).
    pub sink: String,
    pub notes: Vec<String>,
}

/// The whole run's record: env + per-scenario statistics.
#[derive(Debug, Serialize)]
pub struct RunRecord {
    pub run_record_format: &'static str,
    pub task: &'static str,
    pub bench: String,
    pub env: EnvBlock,
    pub scenarios: Vec<ScenarioRecord>,
}

/// Per-sample human line: ns plus an arbitrary suffix. Patch scenarios make
/// `changed < total` explicit in the suffix (per sample, per convention).
pub fn sample_line(scenario: &str, i: usize, n: usize, ns: f64, suffix: &str) {
    println!("[{scenario}] sample {i:>3}/{n}: {ns:>12.0} ns {suffix}");
}

/// Two-phase sampling: per sample, `prepare` builds the workload OUTSIDE the
/// timed region (scene clones, payload clones, indices), `measure` runs the
/// measured operation (it consumes `P` because applying a patch moves the
/// patch), and `suffix` builds the per-sample line AFTER the clock stops.
/// `warmup` untimed prime passes settle caches/allocator state first.
///
/// The measured value is part of the timed work (it is the sink the numbers
/// mean nothing without) but the line printing is not.
pub fn timed_samples<P, T>(
    scenario: &str,
    n: usize,
    warmup: usize,
    mut prepare: impl FnMut() -> P,
    mut measure: impl FnMut(P) -> T,
    mut suffix: impl FnMut(&T) -> String,
) -> Vec<f64> {
    for _ in 0..warmup {
        let value = measure(prepare());
        black_box(&value);
    }
    let mut samples: Vec<f64> = Vec::with_capacity(n);
    for i in 0..n {
        let prepared = prepare();
        let start = Instant::now();
        let value = measure(prepared);
        let ns = start.elapsed().as_nanos() as f64;
        sample_line(scenario, i + 1, n, ns, &suffix(&value));
        samples.push(ns);
    }
    black_box(&samples);
    samples
}

/// Plain timed sampling for ops with no per-sample setup (the op is the
/// whole workload, inputs already built). Same percentiles via
/// [`WallPercentiles::from_samples`].
pub fn timed_ns(n: usize, warmup: usize, mut op: impl FnMut()) -> Vec<f64> {
    for _ in 0..warmup {
        op();
    }
    let mut samples: Vec<f64> = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        op();
        samples.push(start.elapsed().as_nanos() as f64);
    }
    black_box(&samples);
    samples
}

/// The milestone-2 acceptance suffix: makes `changed < total` explicit
/// per sample. `changed` counts the DISTINCT element regions the invalidation
/// plan names (the elements whose prims a per-sync projection would touch);
/// `total` is the full scene size.
pub fn acceptance_line(changed: usize, total: usize) -> String {
    format!(
        "| changed {changed} < total {total} ({:.1}%) ok={}",
        changed as f64 / total.max(1) as f64 * 100.0,
        changed < total
    )
}

/// Print the env lines, assemble the record, print the pretty JSON block,
/// and return the record (the bench drops it — the stdout JSON IS the file).
pub fn emit_run_record(bench: &str, scenarios: Vec<ScenarioRecord>) -> RunRecord {
    let env = env_block(bench);
    println!("\n=== SCN-012 run: {bench} (api-crate pub surface unless labeled otherwise) ===");
    println!(
        "host={} os={} arch={} profile={} kernel={}",
        env.host, env.os, env.arch, env.profile, env.kernel
    );
    println!(
        "rustc={} git_commit={} git_dirty={} timestamp_unix={} pid={}",
        env.rustc, env.git_commit, env.git_dirty, env.timestamp_unix, env.pid
    );
    let record = RunRecord {
        run_record_format: RUN_RECORD_FORMAT,
        task: "SCN-012",
        bench: bench.to_owned(),
        env,
        scenarios,
    };
    println!("\n=== RUN RECORD ===");
    match serde_json::to_string_pretty(&record) {
        Ok(json) => println!("{json}"),
        // Unreachable for derive(Serialize) data, but never panic in a bench.
        Err(e) => println!("{{\"run_record_format\":\"{RUN_RECORD_FORMAT}\",\"serialize_error\":\"{e}\"}}"),
    }
    black_box(&record);
    record
}
