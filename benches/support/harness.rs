use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

// ── Metric Structures ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkMetric {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub samples: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub median: f64,
    pub p95: f64,
    pub p99: f64,
    pub std_dev: f64,
    pub throughput: Option<f64>,
    pub throughput_unit: Option<String>,
    pub target_threshold: Option<f64>,
    pub passed_target: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSuiteReport {
    pub timestamp: String,
    pub os: String,
    pub arch: String,
    pub profile: String,
    pub is_quick_mode: bool,
    pub metrics: Vec<BenchmarkMetric>,
}

pub struct BenchmarkRunner {
    pub title: String,
    pub quick_mode: bool,
    pub filter: Option<String>,
    pub baseline_path: Option<PathBuf>,
    pub output_path: PathBuf,
    pub results: Vec<BenchmarkMetric>,
}

impl BenchmarkRunner {
    pub fn new(title: impl Into<String>, default_output: &str) -> Self {
        let args: Vec<String> = env::args().collect();
        let quick_mode =
            args.iter().any(|a| a == "--quick" || a == "-q") || env::var("CAD_BENCH_QUICK").is_ok();

        let filter = args
            .windows(2)
            .find(|w| w[0] == "--filter")
            .map(|w| w[1].clone())
            .or_else(|| env::var("CAD_BENCH_FILTER").ok());

        let baseline_path = args
            .windows(2)
            .find(|w| w[0] == "--baseline")
            .map(|w| PathBuf::from(&w[1]))
            .or_else(|| env::var("CAD_BENCH_BASELINE").ok().map(PathBuf::from));

        let output_path = args
            .windows(2)
            .find(|w| w[0] == "--output")
            .map(|w| PathBuf::from(&w[1]))
            .or_else(|| env::var("CAD_BENCH_OUTPUT").ok().map(PathBuf::from))
            .unwrap_or_else(|| {
                let target_dir = Path::new("target");
                if !target_dir.exists() {
                    let _ = std::fs::create_dir_all(target_dir);
                }
                target_dir.join(default_output)
            });

        Self {
            title: title.into(),
            quick_mode,
            filter,
            baseline_path,
            output_path,
            results: Vec::new(),
        }
    }

    pub fn should_run(&self, name: &str) -> bool {
        if let Some(ref filter) = self.filter {
            name.to_lowercase().contains(&filter.to_lowercase())
        } else {
            true
        }
    }

    /// Record a measured benchmark with multiple timing samples.
    pub fn record(
        &mut self,
        name: &str,
        description: &str,
        unit: &str,
        mut samples: Vec<f64>,
        throughput: Option<(f64, &str)>,
        target_threshold: Option<f64>,
    ) {
        if samples.is_empty() {
            return;
        }
        samples.sort_by(|a, b| a.total_cmp(b));

        let n = samples.len();
        let min = samples[0];
        let max = samples[n - 1];
        let sum: f64 = samples.iter().sum();
        let mean = sum / (n as f64);
        let median = if n % 2 == 0 {
            (samples[n / 2 - 1] + samples[n / 2]) * 0.5
        } else {
            samples[n / 2]
        };

        let p95_idx = ((n as f64) * 0.95).floor() as usize;
        let p95 = samples[p95_idx.min(n - 1)];

        let p99_idx = ((n as f64) * 0.99).floor() as usize;
        let p99 = samples[p99_idx.min(n - 1)];

        let variance: f64 = samples.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / (n as f64);
        let std_dev = variance.sqrt();

        let passed_target = match target_threshold {
            Some(thresh) => median <= thresh,
            None => true,
        };

        let (thru_val, thru_unit) = match throughput {
            Some((val, u)) => (Some(val), Some(u.to_string())),
            None => (None, None),
        };

        self.results.push(BenchmarkMetric {
            name: name.to_string(),
            description: description.to_string(),
            unit: unit.to_string(),
            samples: n,
            min,
            max,
            mean,
            median,
            p95,
            p99,
            std_dev,
            throughput: thru_val,
            throughput_unit: thru_unit,
            target_threshold,
            passed_target,
        });
    }

    /// Print summary table to stdout and persist JSON report.
    pub fn finish(&self) {
        let baseline_map: HashMap<String, BenchmarkMetric> =
            if let Some(ref path) = self.baseline_path {
                if let Ok(content) = std::fs::read_to_string(path) {
                    if let Ok(report) = serde_json::from_str::<BenchmarkSuiteReport>(&content) {
                        println!(
                            "\nLoaded baseline from '{}' ({} metrics)",
                            path.display(),
                            report.metrics.len()
                        );
                        report
                            .metrics
                            .into_iter()
                            .map(|m| (m.name.clone(), m))
                            .collect()
                    } else {
                        eprintln!(
                            "Warning: Failed to parse baseline JSON at '{}'",
                            path.display()
                        );
                        HashMap::new()
                    }
                } else {
                    eprintln!(
                        "Warning: Could not read baseline file at '{}'",
                        path.display()
                    );
                    HashMap::new()
                }
            } else {
                HashMap::new()
            };

        println!("\n{}", "=".repeat(120));
        println!("{}", self.title);
        println!("{}", "=".repeat(120));
        println!(
            "{:<38} | {:>8} | {:>8} | {:>8} | {:>8} | {:>16} | {:>10} | {:^6}",
            "Benchmark Name", "Median", "Mean", "P95", "Min", "Throughput", "Baseline", "Status"
        );
        println!("------------------------------------------------------------------------------------------------------------------------");

        for m in &self.results {
            let throughput_str = match (&m.throughput, &m.throughput_unit) {
                (Some(val), Some(unit)) => {
                    if *val >= 1_000_000.0 {
                        format!("{:.2}M {}", val / 1_000_000.0, unit)
                    } else if *val >= 1_000.0 {
                        format!("{:.1}k {}", val / 1_000.0, unit)
                    } else {
                        format!("{:.1} {}", val, unit)
                    }
                }
                _ => "-".to_string(),
            };

            let baseline_col = if let Some(base) = baseline_map.get(&m.name) {
                if base.median > 0.0 {
                    let delta_pct = ((m.median - base.median) / base.median) * 100.0;
                    if delta_pct < -1.0 {
                        format!("{:+.1}% (fast)", delta_pct)
                    } else if delta_pct > 1.0 {
                        format!("{:+.1}% (slow)", delta_pct)
                    } else {
                        "~0.0%".to_string()
                    }
                } else {
                    "-".to_string()
                }
            } else {
                "-".to_string()
            };

            let status = if m.passed_target { "PASS" } else { "FAIL" };

            println!(
                "{:<38} | {:>6.2} {:<1} | {:>6.2} {:<1} | {:>6.2} {:<1} | {:>6.2} {:<1} | {:>16} | {:>10} | {:^6}",
                m.name,
                m.median,
                m.unit,
                m.mean,
                m.unit,
                m.p95,
                m.unit,
                m.min,
                m.unit,
                throughput_str,
                baseline_col,
                status,
            );
        }
        println!("========================================================================================================================\n");

        let report = BenchmarkSuiteReport {
            timestamp: chrono_now(),
            os: env::consts::OS.to_string(),
            arch: env::consts::ARCH.to_string(),
            profile: if cfg!(debug_assertions) {
                "debug".to_string()
            } else {
                "release".to_string()
            },
            is_quick_mode: self.quick_mode,
            metrics: self.results.clone(),
        };

        if let Ok(json) = serde_json::to_string_pretty(&report) {
            if let Some(parent) = self.output_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&self.output_path, json) {
                eprintln!(
                    "Error saving metrics to '{}': {}",
                    self.output_path.display(),
                    e
                );
            } else {
                println!(
                    "Saved complete performance metrics JSON to: {}\n",
                    self.output_path.display()
                );
            }
        }
    }
}

fn chrono_now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("unix_{}", d.as_secs())
}
