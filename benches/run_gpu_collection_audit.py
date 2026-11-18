"""Compare saved before/after release audit binaries on actual GPU resources.

Both binaries must contain the unchanged performance_audit_gpu fixture. The
new wire-component audit supplies an additional same-process A/B comparison.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import random
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TEST = "scene::pipeline::gpu_memory::performance_audit::performance_audit_gpu"


def percentile(values, quantile):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * quantile) - 1)]


def bootstrap_interval(deltas):
    rng = random.Random(0x53A7EA1931CB942D)
    medians = sorted(statistics.median(rng.choices(deltas, k=len(deltas)))
                     for _ in range(4000))
    return [medians[100], medians[3899]]


def run(binary, destination, label):
    environment = os.environ.copy()
    environment.pop("PERF", None)
    result = subprocess.run(
        [str(binary), "--ignored", "--exact", TEST, "--nocapture", "--test-threads=1"],
        cwd=ROOT, env=environment, capture_output=True, text=True,
        encoding="utf-8", errors="replace", check=True,
    )
    log = result.stdout + result.stderr
    (destination / f"{label}.log").write_text(log, encoding="utf-8")
    assert "test result: ok. 1 passed; 0 failed" in log
    path = next(line.removeprefix("AUDIT_REPORT ") for line in log.splitlines()
                if line.startswith("AUDIT_REPORT "))
    report = json.loads(Path(path).read_text(encoding="utf-8"))
    assert report["profile"] == "release" and not report["counting_allocator"]
    snapshots = [json.loads(line.removeprefix("[perf] gpu-live-json "))
                 for line in log.splitlines() if line.startswith("[perf] gpu-live-json ")]
    assert len(snapshots) == 6, "expected one complete snapshot for each slot/range fixture"
    return report, snapshots


def fixture_key(case):
    return f"slots={case['slots']},ranges={case['wire_ranges_per_slot']},PERF={case['perf_enabled']}"


def measured(report):
    return {fixture_key(case): case for case in report["results"]["telemetry"]}


def layouts(report):
    return [{key: value for key, value in record.items() if key != "timing"}
            for record in report["results"]["metadata"]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--pairs", type=int, default=10)
    parser.add_argument("--output", type=Path,
                        default=ROOT / "target/performance-audit/gpu-collection-full.json")
    args = parser.parse_args()
    assert args.pairs >= 5
    before, after = args.before.resolve(), args.after.resolve()
    hashes = {"before": hashlib.sha256(before.read_bytes()).hexdigest(),
              "after": hashlib.sha256(after.read_bytes()).hexdigest()}
    assert hashes["before"] != hashes["after"], "save the before executable before rebuilding"
    destination = ROOT / "target/performance-audit/gpu-collection-processes"
    destination.mkdir(parents=True, exist_ok=True)
    rows = {}
    versions = {}
    adapter = None
    snapshots = None
    metadata = None
    for pair in range(args.pairs):
        order = ["before", "after"] if pair % 2 == 0 else ["after", "before"]
        reports = {}
        for arm in order:
            binary = before if arm == "before" else after
            report, state = run(binary, destination, f"pair-{pair:02d}-{arm}")
            if snapshots is None:
                snapshots = state
                adapter = report["results"]["adapter"]
                metadata = layouts(report)
            assert state == snapshots, "GPU totals/categories changed, including sharing or empty state"
            assert report["results"]["adapter"] == adapter
            assert layouts(report) == metadata, "per-record persistent storage changed"
            versions[arm] = {key: report[key] for key in (
                "revision", "profile", "allocator", "os", "arch", "logical_cpus", "counting_allocator"
            )}
            reports[arm] = measured(report)
        assert reports["before"].keys() == reports["after"].keys()
        for key in reports["before"]:
            old, new = reports["before"][key], reports["after"][key]
            assert old["new_covered_bytes"] == new["new_covered_bytes"]
            row = rows.setdefault(key, {
                "slots": old["slots"], "wire_ranges_per_slot": old["wire_ranges_per_slot"],
                "perf_enabled": old["perf_enabled"], "logical_live_bytes": old["new_covered_bytes"],
                "raw_pairs": [],
            })
            # The existing fixture compares partial pre-telemetry counting to
            # full counting. Select its full counter arm in BOTH executables.
            row["raw_pairs"].append({
                "before_samples_us": old["timing"]["after"]["samples"],
                "after_samples_us": new["timing"]["after"]["samples"],
            })
        print(f"Fresh GPU process pair {pair + 1}/{args.pairs}: identical categories and byte counts", flush=True)
    for row in rows.values():
        old = [statistics.median(pair["before_samples_us"]) for pair in row["raw_pairs"]]
        new = [statistics.median(pair["after_samples_us"]) for pair in row["raw_pairs"]]
        row["before"] = {
            "median_us": statistics.median(old), "process_medians_us": old,
            "p95_sample_average_us": percentile(
                [value for pair in row["raw_pairs"] for value in pair["before_samples_us"]], 0.95),
        }
        row["after"] = {
            "median_us": statistics.median(new), "process_medians_us": new,
            "p95_sample_average_us": percentile(
                [value for pair in row["raw_pairs"] for value in pair["after_samples_us"]], 0.95),
        }
        row["median_change_percent"] = (
            (statistics.median(new) / statistics.median(old) - 1) * 100
            if statistics.median(old) else None
        )
        deltas = [b - a for a, b in zip(old, new)]
        row["paired_median_delta_us"] = statistics.median(deltas)
        row["paired_delta_95ci_us"] = bootstrap_interval(deltas)
    result = {
        "schema": 1, "adapter": adapter, "executables": hashes, "versions": versions,
        "process_pairs": args.pairs,
        "scope": "actual report_gpu_live CPU cost with unchanged totals; alternating fresh-process order; each process supplies 100 samples averaging 10 enabled or 1000 disabled calls; headline median is median of process medians; p95 describes pooled sample averages, not individual frame times; includes construction/destruction of collection tables, excludes fixture allocation, GPU draws/presents and changed-state logging",
        "statistics": "pointwise paired-process median-delta 95% bootstrap intervals, 4000 deterministic resamples; not simultaneous bounds",
        "equal_category_snapshots": snapshots, "unchanged_record_layouts": metadata,
        "cases": list(rows.values()),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    for row in result["cases"]:
        if row["perf_enabled"]:
            print(f"{row['slots']} slots / {row['wire_ranges_per_slot']} ranges: "
                  f"{row['before']['median_us']:.3f} -> {row['after']['median_us']:.3f} us")
    print(args.output)


if __name__ == "__main__":
    main()
