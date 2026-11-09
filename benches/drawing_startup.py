#!/usr/bin/env python3
"""Measure a GUI drawing open and process memory on Windows, macOS, or Linux.

The readiness event is the application's PERF open-ui-thread diagnostic, not
first paint or window responsiveness. See benches/README.md for comparison
limits and setup. Only the process started by this script is terminated.
"""

from __future__ import annotations

import argparse
from collections import deque
import hashlib
import json
import os
from pathlib import Path
import platform
import queue
import re
import subprocess
import sys
import tempfile
import threading
import time

try:
    import psutil
except ImportError:
    sys.exit("Install benchmark dependencies: python -m pip install -r benches/requirements.txt")

READY = re.compile(r"^\[perf\] open-ui-thread=(\d+)ms entities=(\d+)")
PHASES = re.compile(r"^\[perf\] open-phases parse=(\d+)ms purge=(\d+)ms xref=(\d+)ms caches=(\d+)ms finalize=(\d+)ms")
WIRES = re.compile(r"^\[perf\] open-finalize .*wire_models=(\d+)")
VERTICES = re.compile(r"^\[perf\] text-upload\s+.*vertices=(\d+)")
INTERESTING = re.compile(r"^\[startup\]|^\[gpu\]|^renderer pipelines:|^\[perf\] (?:open-phases|open-finalize|open-ui-thread|wires-build|text-upload|gpu-live)")


def graphics_settings_path(env: dict[str, str], system: str) -> Path:
    """Match gpu_backend::prefs_path without reading unrelated user settings."""
    if "OCS_GPU_SENTINEL_DIR" in env:
        return Path(env["OCS_GPU_SENTINEL_DIR"]) / "graphics.json"
    if system == "Windows":
        return Path(env["APPDATA"]) / "OpenCADStudio" / "graphics.json"
    if system == "Darwin":
        return Path.home() / "Library" / "Application Support" / "OpenCADStudio" / "graphics.json"
    return Path(env.get("XDG_CONFIG_HOME") or Path.home() / ".config") / "OpenCADStudio" / "graphics.json"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_trace(lines: list[str]) -> dict:
    result = {"entities": None, "wire_models": None, "text_vertices": None,
              "open_phases_ms": None, "gpu_adapter": None}
    for line in lines:
        if match := READY.match(line):
            result["entities"] = int(match[2])
        elif match := PHASES.match(line):
            result["open_phases_ms"] = dict(zip(
                ("parse", "purge", "xref", "caches", "finalize"),
                map(int, match.groups()),
            ))
        elif match := WIRES.match(line):
            result["wire_models"] = int(match[1])
        elif match := VERTICES.match(line):
            result["text_vertices"] = int(match[1])
        elif line.startswith("[gpu] adapter:"):
            result["gpu_adapter"] = line
    return result


def consistent(values: list[object]) -> bool | None:
    if not values or any(value is None for value in values):
        return None
    return all(value == values[0] for value in values[1:])


def run_one(command: list[str], env: dict[str, str], timeout: float,
            settle: float, poll: float) -> dict:
    """Measure one child; always reap it, including on timeout or failure."""
    lines: queue.Queue[tuple[float, str]] = queue.Queue()
    tail: deque[str] = deque(maxlen=30)
    tail_lock = threading.Lock()
    trace: list[str] = []
    started = time.perf_counter()
    proc = subprocess.Popen(command, env=env, stdout=subprocess.DEVNULL,
                            stderr=subprocess.PIPE, text=True, encoding="utf-8",
                            errors="replace", bufsize=1)

    def read_stderr() -> None:
        assert proc.stderr is not None
        for line in proc.stderr:
            line = line.rstrip("\r\n")
            with tail_lock:
                tail.append(line)
            if INTERESTING.match(line):
                lines.put((time.perf_counter(), line))

    reader = threading.Thread(target=read_stderr, daemon=True)
    reader.start()
    sampled_peak_rss = 0
    ready_at = None
    snapshot_rss = None
    snapshot_uss = None
    termination = "already_exited"
    def drain() -> None:
        nonlocal ready_at
        while True:
            try:
                timestamp, line = lines.get_nowait()
            except queue.Empty:
                break
            if len(trace) < 200:
                trace.append(line)
            if ready_at is None and READY.match(line):
                ready_at = timestamp

    def error_tail() -> str:
        with tail_lock:
            return " | ".join(tail)

    try:
        try:
            monitored = psutil.Process(proc.pid)
        except psutil.NoSuchProcess as error:
            raise RuntimeError("Application exited before monitoring began") from error
        while True:
            drain()
            if proc.poll() is not None:
                raise RuntimeError(f"Application exited before sampling completed (exit {proc.returncode}). " + error_tail())
            now = time.perf_counter()
            if ready_at is None and now - started >= timeout:
                raise TimeoutError("Timed out waiting for the drawing-open diagnostic. " + error_tail())
            try:
                sampled_peak_rss = max(sampled_peak_rss, monitored.memory_info().rss)
            except (psutil.NoSuchProcess, psutil.AccessDenied) as error:
                raise RuntimeError(f"Cannot sample the application process: {error}") from error
            if ready_at is not None and now >= ready_at + settle:
                try:
                    snapshot_rss = monitored.memory_info().rss
                except (psutil.NoSuchProcess, psutil.AccessDenied) as error:
                    raise RuntimeError(f"Cannot sample the application process: {error}") from error
                sampled_peak_rss = max(sampled_peak_rss, snapshot_rss)
                try:
                    snapshot_uss = getattr(monitored.memory_full_info(), "uss", None)
                except (psutil.AccessDenied, NotImplementedError, OSError):
                    snapshot_uss = None
                break
            time.sleep(poll)
    finally:
        if proc.poll() is None:
            proc.terminate()
            termination = "terminated"
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
                termination = "killed"
        reader.join(timeout=5)
        if proc.stderr is not None and not reader.is_alive():
            proc.stderr.close()
        elif reader.is_alive():
            print("Warning: a descendant still holds the diagnostic pipe open", file=sys.stderr)
        drain()

    result = parse_trace(trace)
    result.update({"open_event_ms": round((ready_at - started) * 1000, 1),
                   "rss_bytes": snapshot_rss, "uss_bytes": snapshot_uss,
                   "peak_rss_bytes_sampled": sampled_peak_rss,
                   "termination": termination, "trace": trace})
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--drawing", required=True, type=Path)
    parser.add_argument("--executable", required=True, type=Path)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--settle-seconds", type=float, default=15)
    parser.add_argument("--timeout-seconds", type=float, default=180)
    parser.add_argument("--poll-seconds", type=float, default=0.1)
    parser.add_argument("--output", type=Path, default=Path("target/drawing-startup-benchmark.json"))
    args = parser.parse_args()
    if not 1 <= args.runs <= 100 or not 0 < args.settle_seconds <= 600 or not 0 < args.timeout_seconds <= 3600 or not 0.01 <= args.poll_seconds <= 5:
        parser.error("runs, settle, timeout or poll interval is out of range")
    drawing = args.drawing.resolve(strict=True)
    executable = args.executable.resolve(strict=True)
    output = args.output.resolve()
    if not drawing.is_file() or not executable.is_file():
        parser.error("drawing and executable must be files")
    same_path = os.path.normcase(str(output)) in {os.path.normcase(str(drawing)), os.path.normcase(str(executable))}
    same_file = output.exists() and any(os.path.samefile(output, source) for source in (drawing, executable))
    if same_path or same_file:
        parser.error("output must not overwrite the drawing or executable")
    before = [(p.stat().st_size, p.stat().st_mtime_ns) for p in (drawing, executable)]
    settings_path = graphics_settings_path(os.environ, platform.system())
    settings = settings_path.read_bytes() if settings_path.is_file() else None
    settings_hash = hashlib.sha256(settings).hexdigest() if settings is not None else None
    results = []
    for index in range(args.runs):
        # The GPU sentinel override also redirects graphics.json. Copy the
        # original preferences, but never share the user's crash sentinel.
        sentinel_dir = Path(tempfile.mkdtemp(prefix="ocs-drawing-bench-"))
        sentinel_file = sentinel_dir / "gpu_crash_sentinel"
        prefs_file = sentinel_dir / "graphics.json"
        try:
            if settings is not None:
                prefs_file.write_bytes(settings)
            env = os.environ.copy()
            env.update({"PERF": "1", "OCS_STARTUP_LOG": "1", "OCS_GPU_SENTINEL_DIR": str(sentinel_dir)})
            result = run_one([str(executable), "--new-instance", str(drawing)], env,
                             args.timeout_seconds, args.settle_seconds, args.poll_seconds)
        finally:
            sentinel_file.unlink(missing_ok=True)
            prefs_file.unlink(missing_ok=True)
            try:
                sentinel_dir.rmdir()  # empty directory only; never recursively delete
            except OSError as error:
                print(f"Warning: could not remove {sentinel_dir}: {error}", file=sys.stderr)
        result["run"] = index + 1
        results.append(result)
        print(f"Run {index + 1}: open event {result['open_event_ms']:.1f} ms; RSS {result['rss_bytes'] / 2**20:.1f} MiB", flush=True)

    after = [(p.stat().st_size, p.stat().st_mtime_ns) for p in (drawing, executable)]
    if before != after:
        raise RuntimeError("Drawing or executable changed during measurement; results are invalid")
    drawing_hash = sha256(drawing)
    executable_hash = sha256(executable)
    if before != [(p.stat().st_size, p.stat().st_mtime_ns) for p in (drawing, executable)]:
        raise RuntimeError("Drawing or executable changed while hashing; results are invalid")
    adapter_consistent = consistent([run["gpu_adapter"] for run in results])
    workload_consistent = consistent([
        (run["entities"], run["wire_models"], run["text_vertices"])
        if all(run[key] is not None for key in ("entities", "wire_models", "text_vertices")) else None
        for run in results
    ])
    if adapter_consistent is not True or workload_consistent is not True:
        print("Warning: adapter or workload consistency is different or unknown; inspect the report before comparing runs.", file=sys.stderr)
    report = {
        "schema": 2, "benchmark": "drawing_startup", "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "drawing": {"path": str(drawing), "bytes": before[0][0], "sha256": drawing_hash},
        "executable": {"path": str(executable), "bytes": before[1][0], "sha256": executable_hash},
        "system": {"os": platform.system(), "release": platform.release(), "version": platform.version(),
                   "architecture": platform.machine(), "processor": platform.processor(), "logical_processors": os.cpu_count(),
                   "python": platform.python_version(), "psutil": psutil.__version__},
        "settings": {"runs": args.runs, "settle_seconds": args.settle_seconds, "timeout_seconds": args.timeout_seconds,
                     "poll_seconds": args.poll_seconds, "trace": True, "working_directory": str(Path.cwd()),
                     "wgpu_backend": os.environ.get("WGPU_BACKEND"), "shader_disk_cache": os.environ.get("__GL_SHADER_DISK_CACHE"),
                     "shader_disk_cache_path": os.environ.get("__GL_SHADER_DISK_CACHE_PATH"),
                     "gpu_sentinel_policy": "isolated-empty-per-run", "graphics_settings_sha256": settings_hash},
        "adapter_consistent": adapter_consistent, "workload_consistent": workload_consistent, "runs": results,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Saved benchmark report to {output}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError, TimeoutError) as error:
        sys.exit(f"Benchmark failed: {error}")
