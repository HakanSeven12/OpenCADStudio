#!/usr/bin/env python3
"""PERF-off real-drawing GUI replay. See README.md for measurement limits."""

from __future__ import annotations

import argparse
from collections import defaultdict
import csv
import ctypes
import json
import math
import os
from pathlib import Path
import platform
import secrets
import shutil
import socket
import statistics
import subprocess
import threading
import time
import urllib.error
import urllib.request

import psutil

from drawing_startup import graphics_settings_path, sha256

LOCAL_MEASUREMENTS = Path(__file__).resolve().parent / "local_measurements"


def local_output(path):
    path = path.resolve()
    root = LOCAL_MEASUREMENTS.resolve()
    if path == root or not path.is_relative_to(root):
        raise ValueError("Internal measurements must be written inside benches/local_measurements")
    return path


def distribution(values):
    values = sorted(values)
    if not values:
        return None
    return {"count": len(values), "median": statistics.median(values),
            "p95": values[math.ceil(len(values) * .95) - 1], "max": values[-1]}


def recovery_updates(directory, seen, observed_qpc):
    """Record filesystem changes; neither save start nor completion is inferred."""
    updates = []
    for path in directory.glob("*.sv$"):
        try:
            info = path.stat()
        except OSError:
            continue
        fingerprint = (info.st_size, info.st_mtime_ns)
        if seen.get(path.name) != fingerprint:
            seen[path.name] = fingerprint
            updates.append({"observed_qpc": observed_qpc, "name": path.name,
                            "bytes": info.st_size, "mtime_utc_ns": info.st_mtime_ns})
    return updates


def isolation_layout(client, args, layout, active_layout, handle, phase):
    """One controlled workload; fixed-layout controls never recreate panes."""
    if args.scenario == "layout_churn" or layout != active_layout:
        phase("layout", layout, [("commands", {"cmd": "VPORTS " + layout})], args.layout_settle_seconds)
        output = [e["text"] for e in client.call("history")["entries"] if e["kind"] == "Output"]
        expected = {"SINGLE": 1, "2V": 2, "4": 4}[layout]
        if not output or output[-1] != f"VPORTS: {expected} viewport(s).":
            raise RuntimeError("VPORTS did not confirm the isolated layout")
        client.call("commands", cmd="ZOOM EXTENTS")
        active_layout = layout
    if args.scenario == "idle":
        phase("idle", layout, [], args.cycle_seconds)
    elif args.scenario == "state_requests":
        phase("state_requests", layout, [("state", {}) for _ in range(args.steps)])
    elif args.scenario == "edits":
        events = []
        for _ in range(args.edit_steps):
            for cmd in ("MOVE 0,0,0 1,0,0", "MOVE 0,0,0 -1,0,0"):
                events.extend([("select", {"handles": [handle]}), ("commands", {"cmd": cmd})])
        phase("move_retained", layout, events)
    elif args.scenario == "navigation":
        width, height = client.call("state")["viewport_size"]
        center = [width * .25, height * .25]
        pan = [("pointer", {"events": [{"move": center}, {"press": "middle"}]})]
        for n in range(args.steps):
            pan.append(("pointer", {"events": [{"move": [center[0] + 40 * math.sin(2 * math.pi * n / args.steps),
                                                            center[1] + 25 * math.cos(2 * math.pi * n / args.steps)]}]}))
        pan.append(("pointer", {"events": [{"release": "middle"}]}))
        phase("pan", layout, pan)
        zoom = [("pointer", {"events": [{"move": center}, {"wheel": .25 if n % 2 == 0 else -.25}]})
                for n in range(args.steps)]
        phase("zoom", layout, zoom)
    return active_layout


def qpc():
    value = ctypes.c_longlong()
    if not ctypes.windll.kernel32.QueryPerformanceCounter(ctypes.byref(value)):
        raise OSError("QueryPerformanceCounter failed")
    return value.value


def qpc_frequency():
    value = ctypes.c_longlong()
    ctypes.windll.kernel32.QueryPerformanceFrequency(ctypes.byref(value))
    return value.value


def window_state(pid):
    """Read window visibility only; never move, focus or resize a user's window."""
    user32 = ctypes.windll.user32
    user32.GetForegroundWindow.restype = ctypes.c_void_p
    found = []
    callback_type = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_ssize_t)

    @callback_type
    def visit(hwnd, _):
        owner = ctypes.c_ulong()
        user32.GetWindowThreadProcessId(ctypes.c_void_p(hwnd), ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(ctypes.c_void_p(hwnd)):
            found.append(hwnd)
        return True

    user32.EnumWindows(visit, 0)
    foreground = user32.GetForegroundWindow()
    return {"visible_window_count": len(found), "foreground": foreground in found,
            "minimized": any(user32.IsIconic(ctypes.c_void_p(w)) for w in found)}


class Client:
    def __init__(self, port, token, timeout):
        self.base = f"http://127.0.0.1:{port}/api/v1/"
        self.token = token
        self.timeout = timeout
        self.document_id = None
        self.last_click = 0
        self.native = None
        self.serial = 0
        self.request_prefix = "bench-" + secrets.token_hex(8)
        self.raw_counts = defaultdict(int)
        self.click_interval = (ctypes.windll.user32.GetDoubleClickTime() + 50) / 1000
        # Bypass proxies even when the desktop has HTTP_PROXY configured.
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def pace_click(self, op, fields):
        if op == "pointer" and any(e.get("press") == "left" for e in fields.get("events", [])):
            time.sleep(max(0, self.last_click + self.click_interval - time.monotonic()))

    def connect_native(self, config, pid):
        descriptors = [json.loads(p.read_text(encoding="utf-8"))
                       for p in (config / "automation").glob("*.json")]
        matching = [d for d in descriptors if d.get("pid") == pid]
        if len(matching) != 1:
            raise RuntimeError("Expected the owned editor's private automation descriptor")
        self.native = matching[0]

    def raw(self, op, fields):
        self.raw_counts[op] += 1
        if self.native is not None:
            request = dict(fields, protocol=1, op="run" if op == "commands" else op,
                           token=self.native["token"])
            with socket.create_connection(("127.0.0.1", self.native["port"]), self.timeout) as sock:
                sock.settimeout(self.timeout)
                sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                sock.sendall((json.dumps(request) + "\n").encode())
                with sock.makefile("rb") as reader:
                    line = reader.readline(32 * 1024 * 1024)
            if not line.endswith(b"\n"):
                raise RuntimeError("Truncated automation response")
            return json.loads(line)
        body = None if op == "state" else json.dumps(fields).encode()
        request = urllib.request.Request(self.base + op, body, {
            "Authorization": "Bearer " + self.token, "Content-Type": "application/json"})
        try:
            with self.opener.open(request, timeout=self.timeout) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            raise RuntimeError(f"{op}: {error.read().decode(errors='replace')}") from error

    def call(self, op, **fields):
        self.pace_click(op, fields)
        if self.document_id is not None:
            fields.setdefault("document_id", self.document_id)
        self.serial += 1
        fields.setdefault("request_id", f"{self.request_prefix}-{self.serial}")
        result = self.raw(op, fields)
        deadline = time.monotonic() + self.timeout
        while result.get("status") in {"accepted", "running"}:
            if time.monotonic() >= deadline:
                raise TimeoutError(f"{op} did not settle within the timeout")
            time.sleep(.002)
            result = self.raw("operation", {"request_id": fields["request_id"]})
        if result.get("ok") is not True:
            raise RuntimeError(f"{op}: {result}")
        if result.get("status") in {"waiting_input", "accepted", "running", "failed", "cancelled"}:
            raise RuntimeError(f"{op} did not complete: {result}")
        if op == "pointer" and any(e.get("press") == "left" for e in fields.get("events", [])):
            self.last_click = time.monotonic()
        return result


class MemorySampler:
    def __init__(self, pid, interval, recovery_directory=None):
        self.process = psutil.Process(pid)
        self.interval = interval
        self.samples = []
        self.error = None
        self.recovery_directory = recovery_directory
        self.recovery_events = []
        self.recovery_seen = {}
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.run, daemon=True)

    def run(self):
        try:
            while not self.stop.is_set():
                info = self.process.memory_info()
                self.samples.append({"qpc": qpc(), "rss_bytes": info.rss,
                                     "private_commit_bytes": getattr(info, "private", None),
                                     "window": window_state(self.process.pid)})
                if self.recovery_directory is not None:
                    self.recovery_events.extend(recovery_updates(self.recovery_directory, self.recovery_seen, qpc()))
                self.stop.wait(self.interval)
        except (psutil.Error, OSError) as error:
            if not self.stop.is_set():
                self.error = str(error)

    def snapshot(self):
        info = self.process.memory_info()
        try:
            uss = self.process.memory_full_info().uss
        except (psutil.Error, AttributeError, OSError):
            uss = None
        cpu = self.process.cpu_times()
        return {"qpc": qpc(), "rss_bytes": info.rss, "uss_bytes": uss,
                "private_commit_bytes": getattr(info, "private", None),
                "cpu_seconds": cpu.user + cpu.system,
                "threads": self.process.num_threads(), "handles": self.process.num_handles()}


def ps_literal(value):
    return "'" + str(value).replace("'", "''") + "'"


def start_presentmon(executable, pid, artifacts, elevated):
    """Use one uniquely named ETW session; never stop another profiler's session."""
    if executable is None:
        return None
    csv_path = artifacts / "frames.csv"
    log_path = artifacts / "presentmon.log"
    session = "ocs-bench-" + secrets.token_hex(8)
    args = [str(executable), "--process_id", str(pid), "--output_file", str(csv_path),
            "--session_name", session, "--v1_metrics",
            "--qpc_time", "--no_console_stats", "--terminate_on_proc_exit"]
    if elevated:
        # Only the tracer is elevated. The normal editor remains our own child.
        # The controller stops only this helper's trace session, including when
        # the normal editor exits before PresentMon notices the process event.
        script = artifacts / "presentmon.ps1"
        args_literal = ",".join(ps_literal(a) for a in args[1:])
        script.write_text(
            "$ErrorActionPreference='Stop'\n"
            f"$argsForTracer=@({args_literal})\n"
            "$controller = Start-Job -ScriptBlock { param($editorPid, $stop, $exe, $session)\n"
            "  do { Start-Sleep -Milliseconds 200 } while ((Get-Process -Id $editorPid -ErrorAction SilentlyContinue) -and -not (Test-Path -LiteralPath $stop))\n"
            "  Start-Sleep -Seconds 1\n"
            "  & $exe --session_name $session --terminate_existing_session | Out-Null\n"
            "} -ArgumentList " + ",".join(ps_literal(v) for v in
                (pid, artifacts / "presentmon.stop", executable, session)) + "\n"
            "try {\n"
            f"  & {ps_literal(executable)} @argsForTracer *> {ps_literal(log_path)}\n"
            f"  $LASTEXITCODE | Set-Content -LiteralPath {ps_literal(artifacts / 'presentmon.exit')}\n"
            "} catch {\n"
            f"  $_ | Out-String | Set-Content -LiteralPath {ps_literal(log_path)}\n"
            f"  1 | Set-Content -LiteralPath {ps_literal(artifacts / 'presentmon.exit')}\n"
            "} finally { Stop-Job $controller; Remove-Job $controller }\n", encoding="utf-8-sig")
        command = (
            "Start-Process -FilePath 'powershell.exe' -Verb RunAs -WindowStyle Hidden "
            "-ArgumentList " + ps_literal('-NoProfile -ExecutionPolicy Bypass -File "' + str(script) + '"'))
        launcher = subprocess.run(["powershell.exe", "-NoProfile", "-Command", command],
                                  capture_output=True, text=True, timeout=120)
        if launcher.returncode:
            raise RuntimeError("Could not start elevated PresentMon: " + launcher.stderr)
        proc = None
    else:
        with log_path.open("w", encoding="utf-8") as log:
            proc = subprocess.Popen(args, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW)
    deadline = time.monotonic() + 30
    while not csv_path.exists():
        if (proc is not None and proc.poll() is not None) or (artifacts / "presentmon.exit").exists():
            raise RuntimeError("PresentMon failed: " + log_path.read_text(encoding="utf-8-sig", errors="replace"))
        if time.monotonic() > deadline:
            raise TimeoutError("PresentMon did not create its CSV within 30 seconds")
        time.sleep(.1)
    return {"process": proc, "csv": csv_path, "log": log_path, "artifacts": artifacts,
            "elevated": elevated, "executable_sha256": sha256(executable), "session": session,
            "executable": executable}


def finish_presentmon(tracer):
    if tracer is None:
        return {"available": False, "reason": "PresentMon was not requested"}
    # Called after stopping the owned editor; terminate_on_proc_exit flushes CSV.
    proc = tracer["process"]
    (tracer["artifacts"] / "presentmon.stop").touch()
    if proc is not None:
        subprocess.run([str(tracer["executable"]), "--session_name", tracer["session"],
                        "--terminate_existing_session"], stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=10, creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            proc.terminate()
            proc.wait(timeout=5)
            raise RuntimeError("PresentMon did not finish after editor exit")
        exit_code = proc.returncode
    else:
        exit_file = tracer["artifacts"] / "presentmon.exit"
        deadline = time.monotonic() + 20
        while not exit_file.exists() and time.monotonic() < deadline:
            time.sleep(.1)
        if not exit_file.exists():
            raise RuntimeError("Elevated PresentMon did not finish after editor exit")
        exit_code = int(exit_file.read_text(encoding="utf-8-sig").strip())
    if exit_code:
        raise RuntimeError(f"PresentMon exited {exit_code}; inspect {tracer['log']}")
    return {"available": True, "csv": str(tracer["csv"]), "exit_code": exit_code,
            "executable_sha256": tracer["executable_sha256"], "elevated": tracer["elevated"]}


def frame_summaries(path, phases, pid):
    """Only intervals whose two presents lie in the same phase and swap chain."""
    with path.open(encoding="utf-8-sig", newline="") as stream:
        reader = csv.DictReader(stream)
        required = {"ProcessID", "SwapChainAddress", "QPCTime", "msBetweenPresents", "Dropped"}
        if not required.issubset(reader.fieldnames or []):
            raise RuntimeError("Unsupported PresentMon CSV; expected --v1_metrics --qpc_time")
        rows = sorted((r for r in reader if int(r["ProcessID"]) == pid), key=lambda r: int(r["QPCTime"]))
    for phase in phases:
        if "end_qpc" not in phase:
            continue
        chains = defaultdict(list)
        for row in rows:
            if phase["start_qpc"] <= int(row["QPCTime"]) <= phase["end_qpc"]:
                chains[row["SwapChainAddress"]].append(row)
        phase["presentmon"] = []
        for chain, frames in chains.items():
            phase["presentmon"].append({
                "swap_chain": chain, "presents": len(frames),
                "displayed": sum(r["Dropped"] == "0" for r in frames),
                "dropped": sum(r["Dropped"] == "1" for r in frames),
                "between_presents_ms": distribution([float(r["msBetweenPresents"]) for r in frames[1:]]),
                "present_to_display_ms": distribution([
                    float(r["msUntilDisplayed"]) for r in frames
                    if r["Dropped"] == "0" and float(r.get("msUntilDisplayed", "0")) > 0]),
            })
    return {"rows_for_pid": len(rows), "swap_chains": len({r["SwapChainAddress"] for r in rows})}


def assert_state(state):
    if any(state.get(key) for key in ("modal", "command", "operation", "text_editor", "mtext_editor", "block_editor")):
        raise RuntimeError("A modal, command or operation interrupted the workload")
    if any("perf" in str(w).lower() for w in state.get("tool_windows", [])):
        raise RuntimeError("PERF panel is enabled")
    if state.get("mode") != "gui" or not state.get("read_only"):
        raise RuntimeError("Expected a GUI editor with saving disabled")


def prepare_config(artifacts):
    source = Path(os.environ["APPDATA"]) / "OpenCADStudio"
    config = artifacts / "config" / "OpenCADStudio"
    config.mkdir(parents=True)
    hashes = {}
    for name in ("settings.json", "settings.txt", "ocad.pgp", "ocad.pgp.version"):
        if (source / name).is_file():
            if name == "settings.json":
                settings = json.loads((source / name).read_text(encoding="utf-8"))
                if settings.get("settings", {}).get("save_file_path", "").strip():
                    raise RuntimeError("A custom recovery-save destination would bypass config isolation; use settings with a blank save_file_path")
            shutil.copyfile(source / name, config / name)
            hashes[name] = sha256(config / name)
    graphics = graphics_settings_path(os.environ, "Windows")
    if graphics.is_file():
        shutil.copyfile(graphics, config / "graphics.json")
        hashes["graphics.json"] = sha256(config / "graphics.json")
    return config, hashes


def run_one(args, drawing, artifacts, version):
    artifacts.mkdir(parents=True, exist_ok=False)
    config, settings = prepare_config(artifacts)
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    token = secrets.token_hex(32)
    env = {k: v for k, v in os.environ.items() if k.upper() not in {"PERF", "OCS_STARTUP_LOG", "OCS_API_TOKEN"}}
    env.update(APPDATA=str(config.parent), OCS_GPU_SENTINEL_DIR=str(config),
               OPENAEC_BEHEERD="1", OCS_API_TOKEN=token)
    if args.history_max_entries is not None:
        env["OCS_HISTORY_MAX_ENTRIES"] = str(args.history_max_entries)
    client = Client(port, token, args.timeout_seconds)
    log_path = artifacts / "editor.log"
    result = {"drawing_sha256": sha256(drawing), "settings_sha256": settings,
              "phases": [], "checkpoints": [], "valid": False}
    tracer = None
    sampler = None
    started = time.perf_counter()
    with log_path.open("w", encoding="utf-8") as log:
        proc = subprocess.Popen([str(args.executable), "--new-instance", "--read-only",
                                 "--http", str(port), str(drawing)], env=env, stdout=log, stderr=log)
    result["pid"] = proc.pid
    try:
        sampler = MemorySampler(proc.pid, args.memory_poll_seconds, config / "autosave")
        sampler.thread.start()
        deadline = time.monotonic() + args.timeout_seconds
        while True:
            if proc.poll() is not None:
                raise RuntimeError(f"Editor exited {proc.returncode}; inspect {log_path}")
            try:
                state = client.call("state")
            except (urllib.error.URLError, TimeoutError):
                state = {}
            docs = [d for d in state.get("documents", []) if d.get("path") and
                    os.path.normcase(os.path.abspath(d["path"])) == os.path.normcase(str(drawing))]
            if docs and state.get("document_id") == docs[0]["id"] and min(state.get("viewport_size", [0, 0])) > 0:
                break
            if time.monotonic() > deadline:
                raise TimeoutError("Drawing/viewport did not become queryable")
            time.sleep(.1)
        assert_state(state)
        if not version.splitlines()[0].startswith("OpenCADStudio " + state["version"] + "+"):
            raise RuntimeError(f"Live editor {state['version']} differs from CLI build {version}")
        client.document_id = state["document_id"]
        if args.transport == "native":
            client.connect_native(config, proc.pid)
            result["transport"] = "private loopback control; operation polling every 2ms"
        else:
            result["transport"] = "GUI HTTP bridge; server completion polling every 100ms"
        result.update(queryable_ms=(time.perf_counter() - started) * 1000,
                      live_version=state["version"], viewport_size=state["viewport_size"],
                      missing_fonts=state["missing_fonts"], xrefs=state["xrefs"], session_id=state["session_id"])
        result["selection_click_min_interval_ms"] = client.click_interval * 1000
        client.call("commands", cmd="VPORTS SINGLE")
        client.call("commands", cmd="ZOOM EXTENTS")
        if args.autosave_minutes is not None:
            client.call("commands", cmd=f"SAVETIME {args.autosave_minutes}")
        time.sleep(args.settle_seconds)
        capture = client.call("capture", path=str(artifacts / "setup.png"), scope="viewport", max_dimension=1600)["result"]
        owners = {e["owner"] for e in capture["visible_entities"] if e.get("space", "").lower() == "*model_space"}
        if len(owners) != 1:
            raise RuntimeError("Need visible model-space geometry to identify the editing workload")
        owner = owners.pop()
        query = client.call("query", type="Line", detail="geometry", limit=200,
                            where=[{"path": "/owner", "value": owner}])
        result["model_line_count"] = query["count"]
        # Choose a real line with a successful pointer hit and reversible edit.
        line = None
        matrix = capture["world_to_pixel_matrix"]
        for candidate in query["entities"]:
            midpoint = [(a + b) / 2 for a, b in zip(candidate["start"], candidate["end"])]
            x, y = midpoint[:2]
            px = matrix[0] * x + matrix[2] * y + matrix[4]
            py = matrix[1] * x + matrix[3] * y + matrix[5]
            if not (5 <= px < capture["width"] - 5 and 5 <= py < capture["height"] - 5):
                continue
            client.call("select", clear=True)
            hit = client.call("pointer", events=[{"move_world": midpoint}, {"press": "left"}, {"release": "left"}])
            if not hit["state"]["selection"]:
                continue
            client.call("select", clear=True)
            client.call("select", handles=[candidate["handle"]])
            client.call("commands", cmd="MOVE 0,0,0 1,0,0")
            moved = client.call("query", handle=candidate["handle"])["entities"][0]
            if moved["start"][0] != candidate["start"][0] + 1:
                client.call("select", clear=True)
                continue
            client.call("undo")
            client.call("select", clear=True)
            restored = client.call("query", handle=candidate["handle"])["entities"][0]
            if restored["start"] != candidate["start"] or restored["end"] != candidate["end"]:
                raise RuntimeError("Setup edit/undo did not restore the source line")
            line = candidate
            break
        if line is None:
            raise RuntimeError("Could not find a visible, selectable model-space line for replay")
        if args.dirty_start:
            for cmd in ("MOVE 0,0,0 1,0,0", "MOVE 0,0,0 -1,0,0"):
                client.call("select", handles=[line["handle"]])
                client.call("commands", cmd=cmd)
            client.call("select", clear=True)
            restored = client.call("query", handle=line["handle"])["entities"][0]
            if restored["start"] != line["start"] or restored["end"] != line["end"]:
                raise RuntimeError("Dirty-start preparation did not preserve line geometry")
            dirty = next(d["dirty"] for d in client.call("state")["documents"] if d["id"] == client.document_id)
            if not dirty:
                raise RuntimeError("Autosave control requires a dirty drawing")
            result["dirty_at_start"] = True
        result["edit_handle"] = line["handle"]
        result["baseline"] = sampler.snapshot()
        tracer = start_presentmon(args.presentmon, proc.pid, artifacts, args.presentmon_elevated)
        if args.wait_for_foreground_seconds:
            print(f"  waiting for OCS PID {proc.pid} to be foreground", flush=True)
            deadline = time.monotonic() + args.wait_for_foreground_seconds
            while not window_state(proc.pid)["foreground"]:
                if time.monotonic() >= deadline:
                    raise TimeoutError("OCS did not become foreground")
                time.sleep(.2)
        result["baseline"] = sampler.snapshot()
        result["baseline_transport_calls"] = dict(client.raw_counts)
        midpoint = [(a + b) / 2 for a, b in zip(line["start"], line["end"])]
        cycle = 0
        active_layout = "SINGLE"
        session_started = time.monotonic()

        def phase(name, layout, events, idle_seconds=0):
            record = {"name": name, "layout": layout, "cycle": cycle, "start_qpc": qpc(),
                      "window_before": window_state(proc.pid), "requests": []}
            result["phases"].append(record)
            counts_before = dict(client.raw_counts)
            before = client.call("state")
            for op, fields in events:
                client.pace_click(op, fields)
                start_qpc = qpc()
                at = time.perf_counter()
                response = client.call(op, **fields)
                elapsed = (time.perf_counter() - at) * 1000
                record["requests"].append({"op": op, "roundtrip_ms": elapsed,
                                           "start_qpc": start_qpc, "end_qpc": qpc()})
                if name == "selection" and op == "pointer" and not response["state"]["selection"]:
                    raise RuntimeError("Selection replay hit no geometry")
                time.sleep(max(0, args.input_interval_ms / 1000 - elapsed / 1000))
            if idle_seconds:
                time.sleep(idle_seconds)
            after = client.call("state")
            assert_state(after)
            if name in {"pan", "zoom"} and after["camera_revision"] == before["camera_revision"]:
                raise RuntimeError(f"{name} did not change the camera")
            record.update(end_qpc=qpc(), window_after=window_state(proc.pid),
                          viewport_size=after["viewport_size"],
                          camera_revisions=[before["camera_revision"], after["camera_revision"]],
                          roundtrip_ms=distribution([r["roundtrip_ms"] for r in record["requests"]]))
            record["by_operation_ms"] = {
                op: distribution([r["roundtrip_ms"] for r in record["requests"] if r["op"] == op])
                for op in sorted({r["op"] for r in record["requests"]})}
            record["transport_calls"] = {op: count - counts_before.get(op, 0)
                                         for op, count in client.raw_counts.items() if count != counts_before.get(op, 0)}

        def checkpoint(layout):
            restored = client.call("query", handle=line["handle"])["entities"][0]
            if restored["start"] != line["start"] or restored["end"] != line["end"]:
                result["geometry_mismatch"] = {"original": line, "current": restored}
                raise RuntimeError("Replay did not restore line geometry")
            snapshot = sampler.snapshot()
            snapshot.update(cycle=cycle, layout=layout)
            result["checkpoints"].append(snapshot)
            (artifacts / "progress.json").write_text(json.dumps(result["checkpoints"], indent=2), encoding="utf-8")
            print(f"  cycle {cycle} layout {layout}: RSS {snapshot['rss_bytes'] / 2**20:.1f} MiB; commit {snapshot['private_commit_bytes'] / 2**20:.1f} MiB", flush=True)

        while True:
            cycle_started = time.monotonic()
            for layout in args.layouts:
                if args.scenario != "full":
                    active_layout = isolation_layout(client, args, layout, active_layout, line["handle"], phase)
                    checkpoint(layout)
                    continue
                client.call("commands", cmd="VPORTS " + layout)
                history = client.call("history")["entries"]
                output = [entry["text"] for entry in history if entry["kind"] == "Output"]
                expected = {"SINGLE": 1, "2V": 2, "4": 4}[layout]
                if not output or output[-1] != f"VPORTS: {expected} viewport(s).":
                    raise RuntimeError("VPORTS did not confirm the requested viewport count")
                client.call("commands", cmd="ZOOM EXTENTS")
                time.sleep(args.layout_settle_seconds)
                state = client.call("state")
                width, height = state["viewport_size"]
                center = [width * .25, height * .25]
                # Separate requests allow redraws between pointer events.
                pan = [("pointer", {"events": [{"move": center}, {"press": "middle"}]})]
                for n in range(args.steps):
                    pan.append(("pointer", {"events": [{"move": [center[0] + 40 * math.sin(2 * math.pi * n / args.steps),
                                                                   center[1] + 25 * math.cos(2 * math.pi * n / args.steps)]}]}))
                pan.append(("pointer", {"events": [{"release": "middle"}]}))
                phase("pan", layout, pan)
                client.call("commands", cmd="ZOOM EXTENTS")
                zoom = [("pointer", {"events": [{"move": center}, {"wheel": .25 if n % 2 == 0 else -.25}]})
                        for n in range(args.steps)]
                phase("zoom", layout, zoom)
                client.call("commands", cmd="ZOOM EXTENTS")
                selection = []
                for _ in range(args.edit_steps):
                    selection.extend([("select", {"clear": True}), ("pointer", {"events": [
                        {"move_world": midpoint}, {"press": "left"}, {"release": "left"}]})])
                phase("selection", layout, selection)
                client.call("select", clear=True)
                client.call("select", handles=[line["handle"]])
                edits = []
                for _ in range(args.edit_steps):
                    edits.extend([("select", {"handles": [line["handle"]]}),
                                  ("commands", {"cmd": "MOVE 0,0,0 1,0,0"}),
                                  ("undo", {})])
                phase("move_undo", layout, edits)
                retained = []
                for _ in range(args.edit_steps):
                    for cmd in ("MOVE 0,0,0 1,0,0", "MOVE 0,0,0 -1,0,0"):
                        retained.append(("select", {"handles": [line["handle"]]}))
                        retained.append(("commands", {"cmd": cmd}))
                phase("move_retained", layout, retained)
                restored = client.call("query", handle=line["handle"])["entities"][0]
                if restored["start"] != line["start"] or restored["end"] != line["end"]:
                    result["geometry_mismatch"] = {"original": line, "current": restored}
                    raise RuntimeError("Timed edit/undo did not restore line geometry")
                snapshot = sampler.snapshot()
                snapshot.update(cycle=cycle, layout=layout)
                result["checkpoints"].append(snapshot)
                print(f"  cycle {cycle} layout {layout}: RSS {snapshot['rss_bytes'] / 2**20:.1f} MiB", flush=True)
            time.sleep(max(0, args.cycle_seconds - (time.monotonic() - cycle_started)))
            cycle += 1
            if (args.cycles and cycle >= args.cycles) or (not args.cycles and time.monotonic() - session_started >= args.session_seconds):
                break
        result["active_seconds"] = time.monotonic() - session_started
        client.call("commands", cmd="VPORTS SINGLE")
        client.call("commands", cmd="ZOOM EXTENTS")
        time.sleep(args.settle_seconds)
        result["final"] = sampler.snapshot()
        result["final_state"] = client.call("state")
        result["final_transport_calls"] = dict(client.raw_counts)
        result["cycles"] = cycle
        result["all_phase_boundaries_foreground"] = all(
            p[key]["foreground"] and not p[key]["minimized"]
            for p in result["phases"] for key in ("window_before", "window_after"))
        for p in result["phases"]:
            windows = [s["window"] for s in sampler.samples if p["start_qpc"] <= s["qpc"] <= p["end_qpc"]]
            p["foreground_polls"] = len(windows)
            p["all_polls_foreground"] = all(w["foreground"] and not w["minimized"] for w in windows) if windows else None
        result["valid"] = True
    except Exception as error:
        result["error"] = f"{type(error).__name__}: {error}"
    finally:
        if sampler is not None:
            sampler.stop.set()
            sampler.thread.join(timeout=5)
            result["memory_samples"] = sampler.samples
            result["recovery_events"] = sampler.recovery_events
            result["memory_sampler_error"] = sampler.error
            if sampler.error:
                result["valid"] = False
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=10)
                result["termination"] = "terminated-owned-editor"
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
                result["termination"] = "killed-owned-editor"
        else:
            result["termination"] = f"already-exited-{proc.returncode}"
        try:
            result["frame_capture"] = finish_presentmon(tracer)
            if tracer:
                result["frame_capture"].update(frame_summaries(tracer["csv"], result["phases"], proc.pid))
        except Exception as error:
            result["frame_capture"] = {"available": False, "error": str(error)}
            result["valid"] = False
        log = log_path.read_text(encoding="utf-8", errors="replace")
        result["gpu_adapter"] = next((s for s in log.splitlines() if s.startswith("[gpu] adapter:")), None)
        result["perf_log_lines"] = sum(s.startswith("[perf]") for s in log.splitlines())
        if result["perf_log_lines"]:
            result["valid"] = False
        (artifacts / "result.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", required=True, type=Path)
    parser.add_argument("--drawing", required=True, action="append", type=Path)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--scenario", choices=["full", "idle", "layout_churn", "edits", "navigation", "state_requests"], default="full")
    parser.add_argument("--transport", choices=["native", "http"], default="native")
    parser.add_argument("--cycles", type=int, default=0, help="Fixed cycle count; overrides session duration")
    parser.add_argument("--cycle-seconds", type=float, default=0, help="Minimum cycle duration for paced controls")
    parser.add_argument("--history-max-entries", type=int)
    parser.add_argument("--autosave-minutes", type=int)
    parser.add_argument("--dirty-start", action="store_true", help="Preserve geometry but mark the owned drawing dirty before timing")
    parser.add_argument("--session-seconds", type=float, default=900)
    parser.add_argument("--steps", type=int, default=60)
    parser.add_argument("--edit-steps", type=int, default=12)
    parser.add_argument("--layouts", nargs="+", choices=["SINGLE", "2V", "4"], default=["SINGLE", "2V", "4"])
    parser.add_argument("--input-interval-ms", type=float, default=33.333)
    parser.add_argument("--memory-poll-seconds", type=float, default=.25)
    parser.add_argument("--settle-seconds", type=float, default=10)
    parser.add_argument("--layout-settle-seconds", type=float, default=1)
    parser.add_argument("--timeout-seconds", type=float, default=180)
    parser.add_argument("--presentmon", type=Path)
    parser.add_argument("--presentmon-elevated", action="store_true")
    parser.add_argument("--wait-for-foreground-seconds", type=float, default=0)
    parser.add_argument("--expected-revision")
    parser.add_argument("--output", type=Path, default=LOCAL_MEASUREMENTS / "drawing-interaction.json")
    args = parser.parse_args()
    if platform.system() != "Windows":
        parser.error("This harness uses Windows QPC, window state and optional PresentMon")
    if not (1 <= args.runs <= 100 and 0 <= args.session_seconds <= 86400 and 2 <= args.steps <= 10000
            and 1 <= args.edit_steps <= 1000 and 0 <= args.input_interval_ms <= 1000
            and .05 <= args.memory_poll_seconds <= 5 and 0 <= args.settle_seconds <= 600
            and 0 <= args.layout_settle_seconds <= 60 and 1 <= args.timeout_seconds <= 3600
            and 0 <= args.wait_for_foreground_seconds <= 600):
        parser.error("Invalid run count or timing/step limit")
    if args.presentmon_elevated and not args.presentmon:
        parser.error("--presentmon-elevated requires --presentmon")
    if not 0 <= args.cycles <= 100000 or not 0 <= args.cycle_seconds <= 3600:
        parser.error("Invalid isolation cycle limit")
    if args.scenario in {"idle", "edits", "navigation", "state_requests"} and len(args.layouts) != 1:
        parser.error("Fixed-layout isolation requires exactly one --layouts value")
    if args.scenario == "idle" and args.cycle_seconds <= 0:
        parser.error("Idle isolation requires a positive --cycle-seconds")
    if args.history_max_entries is not None and not 1 <= args.history_max_entries <= 100000:
        parser.error("History entry limit must be positive")
    if args.autosave_minutes is not None and not 0 <= args.autosave_minutes <= 1440:
        parser.error("Invalid autosave interval")
    args.executable = args.executable.resolve(strict=True)
    args.drawing = [p.resolve(strict=True) for p in args.drawing]
    if args.presentmon:
        args.presentmon = args.presentmon.resolve(strict=True)
    try:
        args.output = local_output(args.output)
    except ValueError as error:
        parser.error(str(error))
    if args.output.exists():
        parser.error("Output must be a new file")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    artifacts = args.output.with_suffix(".artifacts")
    if artifacts.exists():
        parser.error("Artifacts directory must be new")
    sources = [args.executable, *args.drawing]
    fingerprints = [(p.stat().st_size, p.stat().st_mtime_ns, sha256(p)) for p in sources]
    version = subprocess.check_output([str(args.executable), "--version"], text=True, timeout=15).strip()
    if args.expected_revision and f"revision: {args.expected_revision}" not in version:
        parser.error("Executable revision does not match --expected-revision")
    report = {"schema": 1, "benchmark": "drawing_interaction", "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "harness_sha256": sha256(Path(__file__)),
              "executable": {"path": str(args.executable), "version": version, "sha256": fingerprints[0][2]},
              "drawings": [{"path": str(p), "bytes": f[0], "sha256": f[2]} for p, f in zip(args.drawing, fingerprints[1:])],
              "system": {"os": platform.platform(), "cpu": platform.processor(), "logical_processors": os.cpu_count(),
                         "python": platform.python_version(), "psutil": psutil.__version__, "qpc_frequency": qpc_frequency()},
              "settings": {k: v for k, v in vars(args).items() if k not in {"executable", "drawing", "output", "presentmon"}},
              "perf": False, "metric": "automation request roundtrip, not input-to-display latency",
              "runs": []}
    try:
        for n in range(args.runs):
            for i, drawing in enumerate(args.drawing):
                print(f"Drawing {i + 1}, run {n + 1}: {version.splitlines()[0]}", flush=True)
                result = run_one(args, drawing, artifacts / f"drawing-{i+1}-run-{n+1}", version)
                report["runs"].append(result)
                args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
                if not result["valid"]:
                    raise RuntimeError(f"Invalid run: {result.get('error', result.get('frame_capture'))}")
        report["sources_unchanged"] = fingerprints == [(p.stat().st_size, p.stat().st_mtime_ns, sha256(p)) for p in sources]
        if not report["sources_unchanged"]:
            raise RuntimeError("Drawing or executable changed during measurement")
    finally:
        args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Saved {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
