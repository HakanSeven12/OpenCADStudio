"""Run with python3 scripts/test_release.py; uses only local temporary repositories."""

from contextlib import redirect_stdout
from datetime import datetime, timezone
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import release


class ReleaseTests(unittest.TestCase):
    def test_build_metadata(self):
        with tempfile.TemporaryDirectory() as temp:
            binary = str(Path(temp) / "build-script")
            subprocess.run(["rustc", str(Path(__file__).resolve().parents[1] / "build.rs"), "-o", binary], check=True)
            for cargo in ("0.9.8", "2026.9.0", "2026.35.0", "2026.53.0", "2027.1.0"):
                result = subprocess.check_output([binary], text=True, env={**os.environ, "CARGO_PKG_VERSION": cargo})
                self.assertIn(f"cargo:rustc-env=OCS_APP_VERSION={release.display_version(cargo)}\n", result)

    def test_calendar_versions(self):
        self.assertEqual(release.versions("v2026.35"), {
            "version": "2026.35", "cargo": "2026.35.0", "msi": "26.35.0", "tag": "v2026.35",
        })
        self.assertEqual(release.versions("2026.09")["cargo"], "2026.9.0")
        self.assertEqual(release.display_version("2026.9.0"), "2026.09")
        self.assertEqual(release.display_version("0.9.8"), "0.9.8")
        self.assertEqual(datetime(2027, 1, 3, tzinfo=timezone.utc).strftime("v%G.%V"), "v2026.53")
        for value in ("2026.00", "2026.54", "2027.53", "2026.9", "2026.35.0", "bad", "0.09.8"):
            with self.assertRaises(ValueError, msg=value):
                release.versions(value)

    def test_release_commit_push_and_retry(self):
        original_run = release.run
        releases = {"v0.9.8": {"name": "v0.9.8", "body": "Previous notes", "isDraft": False}}
        latest = "v0.9.8"
        fail_create = False

        def run(*args):
            nonlocal latest, fail_create
            if args[0] != "gh":
                return original_run(*args)
            if args[1:3] == ("release", "list"):
                return json.dumps([{"tagName": tag, "isDraft": False} for tag in releases])
            if args[1:3] == ("release", "create"):
                if fail_create:
                    fail_create = False
                    raise RuntimeError("Temporary release API failure")
                latest = args[3]
                releases[latest] = {
                    "name": args[args.index("--title") + 1],
                    "body": Path(args[args.index("--notes-file") + 1]).read_text(encoding="utf-8"),
                    "isDraft": False,
                }
                return ""
            self.assertEqual(args[1:3], ("release", "view"))
            if args[3] == "--json":
                return json.dumps({"tagName": latest})
            return json.dumps(releases[args[3]])

        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            root = temp / "checkout"
            root.mkdir()
            previous_directory = Path.cwd()
            os.chdir(root)
            try:
                original_run("git", "init", "-b", "main")
                original_run("git", "config", "user.name", "Release test")
                original_run("git", "config", "user.email", "test@example.invalid")
                original_run("git", "init", "--bare", "-b", "main", str(temp / "origin.git"))
                original_run("git", "remote", "add", "origin", str(temp / "origin.git"))
                Path("Cargo.toml").write_text('[package]\nname = "OpenCADStudio"\nversion = "0.9.8"\n')
                Path("Cargo.lock").write_text('version = 4\n[[package]]\nname = "OpenCADStudio"\nversion = "0.9.8"\n')
                original_run("git", "add", ".")
                original_run("git", "commit", "-m", "Initial release")
                original_run("git", "tag", "v0.9.8")
                original_run("git", "push", "origin", "main", "--tags")

                with patch.object(release, "run", run), patch.object(release, "datetime") as clock, patch.dict(os.environ, {
                    "GITHUB_REPOSITORY": "owner/repo", "GITHUB_REF": "refs/heads/main", "GITHUB_OUTPUT": str(temp / "output"),
                }):
                    clock.now.return_value = datetime(2026, 8, 30, 12, tzinfo=timezone.utc)

                    def prepare(publish):
                        result = io.StringIO()
                        with redirect_stdout(result):
                            release.prepare(publish)
                        return result.getvalue()

                    self.assertIn("ready=false", prepare(True))
                    Path("feature").write_text("web release source")
                    original_run("git", "add", "feature")
                    original_run("git", "commit", "-m", "Add web release synchronization")
                    preview = prepare(False)
                    self.assertIn("Add web release synchronization", preview)
                    self.assertEqual(release.cargo_version(), "0.9.8")
                    self.assertEqual(original_run("git", "tag", "--list", "v2026.35"), "")

                    self.assertIn("ready=true", prepare(True))
                    sha = original_run("git", "rev-parse", "HEAD")
                    self.assertEqual(release.cargo_version(), "2026.35.0")
                    self.assertIn('version = "2026.35.0"', Path("Cargo.lock").read_text())
                    self.assertEqual(original_run("git", "log", "-1", "--format=%s"), "Release v2026.35")
                    self.assertEqual(original_run("git", "rev-parse", "origin/main"), sha)
                    self.assertEqual(original_run("git", "rev-parse", "v2026.35^{commit}"), sha)
                    self.assertEqual(original_run("git", "status", "--porcelain"), "")
                    self.assertIn(f"commit={sha}", prepare(True))

                    clock.now.return_value = datetime(2026, 9, 6, 12, tzinfo=timezone.utc)
                    self.assertIn("ready=false", prepare(True))
                    Path("feature").write_text("next change")
                    original_run("git", "add", "feature")
                    original_run("git", "commit", "-m", "Fix release retry")
                    fail_create = True
                    with self.assertRaises(RuntimeError):
                        prepare(True)
                    sha = original_run("git", "rev-parse", "HEAD")
                    self.assertIn(f"commit={sha}", prepare(True))
                    self.assertEqual(original_run("git", "rev-parse", "HEAD"), sha)
                    self.assertEqual(releases["v2026.36"]["name"], "2026.36")
            finally:
                os.chdir(previous_directory)


if __name__ == "__main__":
    unittest.main()
