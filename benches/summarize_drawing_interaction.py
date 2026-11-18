#!/usr/bin/env python3
"""Summarize GUI reports for the ignored internal measurement folder only."""
import argparse
from bisect import bisect_right
from collections import defaultdict
import csv
import json
from pathlib import Path, PureWindowsPath
import re

from drawing_interaction import distribution, local_output
from drawing_startup import sha256


def artifact_csv(report_path, recorded):
    """Prefer the report's adjacent artifacts after moving a local collection."""
    parts = PureWindowsPath(recorded).parts if "\\" in recorded else Path(recorded).parts
    if len(parts) >= 2 and re.fullmatch(r"drawing-\d+-run-\d+", parts[-2]) and parts[-1] == "frames.csv":
        adjacent = report_path.with_suffix(".artifacts") / parts[-2] / parts[-1]
        if adjacent.is_file():
            return adjacent
    original = Path(recorded)
    if original.is_file():
        return original
    raise FileNotFoundError("Frame CSV is missing from the recorded or adjacent artifact directory")


def summarize(path):
    report = json.loads(path.read_text(encoding="utf-8"))
    if report.get("schema") != 1 or report.get("benchmark") != "drawing_interaction":
        raise ValueError("Unsupported interaction report")
    if report.get("sources_unchanged") is not True:
        raise ValueError("Input/executable stability was not verified")
    summary = {"schema": 1, "benchmark": "drawing_interaction_summary", "source_sha256": sha256(path),
               "harness_sha256": report["harness_sha256"], "timestamp_utc": report["timestamp_utc"],
               "executable": {k: report["executable"][k] for k in ("version", "sha256")},
               "drawings": [{k: d[k] for k in ("bytes", "sha256")} for d in report["drawings"]],
               "system": report["system"], "settings": report["settings"], "runs": []}
    for run in report["runs"]:
        if not run["valid"] or run["perf_log_lines"] != 0 or run["memory_sampler_error"]:
            raise ValueError("Cannot summarize an invalid/PERF-on run")
        phases = run["phases"]
        requests = defaultdict(list)
        for phase in phases:
            for request in phase["requests"]:
                requests[(phase["layout"], phase["name"], request["op"])].append(request["roundtrip_ms"])
        requests = [{"layout": layout, "phase": phase, "operation": op, "roundtrip_ms": distribution(values)}
                    for (layout, phase, op), values in sorted(requests.items())]
        result = {k: run[k] for k in ("drawing_sha256", "live_version", "transport", "viewport_size", "gpu_adapter",
                  "settings_sha256", "selection_click_min_interval_ms", "model_line_count", "cycles", "active_seconds",
                  "all_phase_boundaries_foreground", "termination", "perf_log_lines")}
        result["all_phase_polls_foreground"] = all(p["all_polls_foreground"] is True for p in phases)
        result["dirty_at_start"] = run.get("dirty_at_start", False)
        result["recovery_updates"] = [
            {"elapsed_seconds": (event["observed_qpc"] - run["baseline"]["qpc"]) / report["system"]["qpc_frequency"],
             "bytes": event["bytes"]}
            for event in run.get("recovery_events", [])]
        timed = [(phase, request) for phase in phases for request in phase["requests"]
                 if "start_qpc" in request]
        result["slowest_requests"] = [
            {"layout": phase["layout"], "phase": phase["name"], "operation": request["op"],
             "roundtrip_ms": request["roundtrip_ms"],
             "elapsed_seconds": (request["start_qpc"] - run["baseline"]["qpc"]) / report["system"]["qpc_frequency"]}
            for phase, request in sorted(timed, key=lambda pair: pair[1]["roundtrip_ms"], reverse=True)[:10]]
        # Do not publish entity handles, document state, private paths or screenshots.
        result["request_metrics"] = requests
        result["memory"] = {
            "baseline": {k: v for k, v in run["baseline"].items() if k != "qpc"},
            "final": {k: v for k, v in run["final"].items() if k != "qpc"},
            "peak_sampled_rss_bytes": max(s["rss_bytes"] for s in run["memory_samples"]),
            "checkpoints": [dict({k: v for k, v in s.items() if k != "qpc"},
                                 elapsed_seconds=(s["qpc"] - run["baseline"]["qpc"]) / report["system"]["qpc_frequency"])
                            for s in run["checkpoints"]]}
        capture = run["frame_capture"]
        result["frame_capture"] = {k: v for k, v in capture.items() if k != "csv"}
        result["frame_metrics"] = None
        if capture["available"]:
            csv_path = artifact_csv(path, capture["csv"])
            result["frame_capture"]["csv_sha256"] = sha256(csv_path)
            starts = [p["start_qpc"] for p in phases]
            seen = set()
            groups = defaultdict(lambda: {"presents": 0, "displayed": 0, "dropped": 0,
                                          "intervals": [], "display_delays": []})
            chains = {}
            modes = defaultdict(int)
            with csv_path.open(encoding="utf-8-sig", newline="") as stream:
                for frame in csv.DictReader(stream):
                    if int(frame["ProcessID"]) != run["pid"]:
                        continue
                    at = int(frame["QPCTime"])
                    index = bisect_right(starts, at) - 1
                    if index < 0 or at > phases[index]["end_qpc"]:
                        continue
                    phase = phases[index]
                    chain = frame["SwapChainAddress"]
                    chains.setdefault(chain, len(chains) + 1)
                    key = (phase["layout"], phase["name"], chains[chain])
                    group = groups[key]
                    group["presents"] += 1
                    group["displayed"] += frame["Dropped"] == "0"
                    group["dropped"] += frame["Dropped"] == "1"
                    modes[frame["PresentMode"]] += 1
                    if (index, chain) in seen:
                        group["intervals"].append(float(frame["msBetweenPresents"]))
                    seen.add((index, chain))
                    if frame["Dropped"] == "0" and float(frame["msUntilDisplayed"]) > 0:
                        group["display_delays"].append(float(frame["msUntilDisplayed"]))
            result["frame_metrics"] = []
            for (layout, phase, chain), group in sorted(groups.items()):
                intervals = group.pop("intervals")
                delays = group.pop("display_delays")
                result["frame_metrics"].append(dict(group, layout=layout, phase=phase, swap_chain=chain,
                    between_presents_ms=distribution(intervals), present_to_display_ms=distribution(delays)))
            result["frame_capture"]["phase_present_modes"] = dict(modes)
        summary["runs"].append(result)
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        args.output = local_output(args.output)
    except ValueError as error:
        parser.error(str(error))
    if args.output.exists():
        parser.error("Output must be a new file")
    result = summarize(args.report)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"Saved {args.output}")


if __name__ == "__main__":
    main()
