"""Run after the opt-in release audit; reuse its exact built dependencies."""
from pathlib import Path
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "target"
FINGERPRINTS = TARGET / "release" / ".fingerprint"


def main():
    metadata = sorted(FINGERPRINTS.glob("OpenCADStudio-*/test-lib-OpenCADStudio.json"),
                      key=lambda path: path.stat().st_mtime, reverse=True)
    metadata = next(path for path in metadata
                    if "performance-audit" in json.loads(path.read_text())["features"])
    dependencies = json.loads(metadata.read_text())["deps"]
    directory = TARGET / "performance-audit"
    directory.mkdir(exist_ok=True)
    snapshots = [json.loads(line.removeprefix("[perf] gpu-live-json "))
                 for line in (TARGET / "performance-audit-release.log").read_text().splitlines()
                 if line.startswith("[perf] gpu-live-json ")]
    input_path = directory / "logging-input.json"
    input_path.write_text(json.dumps(snapshots[-1]))
    binary = directory / ("gpu-logging.exe" if os.name == "nt" else "gpu-logging")
    command = ["rustc", "--edition", "2021", "-O",
               str(ROOT / "benches/support/gpu_logging_audit.rs"),
               "-L", f"dependency={TARGET / 'release/deps'}", "-o", str(binary)]
    for package in ["iced", "serde", "serde_json", "mimalloc"]:
        expected = next((dep[3] for dep in dependencies if dep[1] == package), None)
        if expected is None:
            continue
        match = next(path for path in FINGERPRINTS.glob(f"{package}-*/lib-{package}")
                     if int.from_bytes(bytes.fromhex(path.read_text().strip()), "little") == expected)
        command += ["--extern", f"{package}={TARGET / 'release/deps' / ('lib' + match.parent.name + '.rlib')}"]
    for path in (TARGET / "release/build").glob("*/out"):
        command += ["-L", f"native={path}"]
    for path in (Path.home() / ".cargo/registry/src").glob("*/windows_x86_64_gnu-*/lib"):
        command += ["-L", f"native={path}"]
    environment = os.environ.copy()
    environment["CARGO_MANIFEST_DIR"] = str(ROOT)
    revision = subprocess.check_output(["git", "rev-parse", "--short=12", "HEAD"], cwd=ROOT, text=True).strip()
    if subprocess.run(["git", "diff-index", "--quiet", "HEAD", "--"], cwd=ROOT).returncode:
        revision += "-dirty"
    environment["OCS_GIT_REV"] = revision
    environment.pop("PERF", None)
    subprocess.run(command, cwd=ROOT, env=environment, check=True)
    output = subprocess.run([str(binary), str(input_path)], cwd=ROOT, env=environment,
                            capture_output=True, text=True, check=True)
    (directory / "logging-output.log").write_text(output.stdout + output.stderr)
    result = json.loads((directory / "gpu-logging.json").read_text())["results"]["timing"]
    print(f"Changed-state format/enqueue: {result['before']['median_us']:.3f} -> "
          f"{result['after']['median_us']:.3f} us median; "
          f"{result['before']['p95_us']:.3f} -> {result['after']['p95_us']:.3f} us p95")


if __name__ == "__main__":
    main()
