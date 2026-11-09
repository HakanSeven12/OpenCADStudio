"""Tests for the cross-platform startup benchmark; no GUI is required."""

import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


BENCHMARK = Path(__file__).with_name("drawing_startup.py")
WORKSPACE = BENCHMARK.resolve().parents[1]
TEST_ROOT = (WORKSPACE / "target").resolve()
spec = importlib.util.spec_from_file_location("drawing_startup", BENCHMARK)
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class DrawingStartupTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not TEST_ROOT.is_relative_to(WORKSPACE):
            raise RuntimeError("test output must stay inside the workspace")
        TEST_ROOT.mkdir(exist_ok=True)

    def test_parses_real_open_diagnostics(self):
        parsed = bench.parse_trace([
            "[perf] open-phases parse=33ms purge=0ms xref=0ms caches=2ms finalize=831ms",
            "[perf] open-finalize wires=824.2ms index=3.0ms entity_index=2.2ms wire_models=10000",
            "[perf] open-ui-thread=3ms entities=10000",
            "[perf] text-upload     5.4ms vertices=209670 atlas=3.2ms plain=5.0ms block-batches=0",
            "[gpu] adapter: example (Vulkan)",
        ])
        self.assertEqual(parsed["open_phases_ms"], {"parse": 33, "purge": 0, "xref": 0, "caches": 2, "finalize": 831})
        self.assertEqual((parsed["entities"], parsed["wire_models"], parsed["text_vertices"]), (10000, 10000, 209670))
        self.assertEqual(parsed["gpu_adapter"], "[gpu] adapter: example (Vulkan)")

    def test_consistency_does_not_treat_missing_data_as_equal(self):
        self.assertTrue(bench.consistent([(1, 2), (1, 2)]))
        self.assertFalse(bench.consistent([(1, 2), (1, 3)]))
        self.assertIsNone(bench.consistent([None, None]))
        self.assertIsNone(bench.consistent([]))

    def test_graphics_preferences_follow_platform_config_paths(self):
        self.assertEqual(bench.graphics_settings_path({"OCS_GPU_SENTINEL_DIR": "custom"}, "Windows"),
                         Path("custom") / "graphics.json")
        self.assertEqual(bench.graphics_settings_path({"APPDATA": "config"}, "Windows"),
                         Path("config") / "OpenCADStudio" / "graphics.json")
        with patch.object(bench.Path, "home", return_value=Path("home")):
            self.assertEqual(bench.graphics_settings_path({}, "Darwin"),
                             Path("home") / "Library" / "Application Support" / "OpenCADStudio" / "graphics.json")
            self.assertEqual(bench.graphics_settings_path({}, "Linux"),
                             Path("home") / ".config" / "OpenCADStudio" / "graphics.json")
        self.assertEqual(bench.graphics_settings_path({"XDG_CONFIG_HOME": "config"}, "Linux"),
                         Path("config") / "OpenCADStudio" / "graphics.json")

    def test_measures_and_reaps_child_after_open_event(self):
        child = ("import sys,time; "
                 "sys.stderr.write('[perf] open-ui-thread=3ms entities=10\\n'); "
                 "sys.stderr.flush(); time.sleep(10)")
        result = bench.run_one([sys.executable, "-c", child], os.environ.copy(), 5, 0.1, 0.02)
        self.assertEqual(result["entities"], 10)
        self.assertGreater(result["rss_bytes"], 0)
        self.assertGreaterEqual(result["peak_rss_bytes_sampled"], result["rss_bytes"])
        self.assertGreater(result["open_event_ms"], 0)
        self.assertIn(result["termination"], ("terminated", "killed"))

    def test_timeout_reaps_child(self):
        with tempfile.TemporaryDirectory(dir=TEST_ROOT) as directory:
            self.assertTrue(Path(directory).resolve().is_relative_to(TEST_ROOT))
            pidfile = Path(directory) / "pid"
            child = ("import os,sys,time; "
                     "open(sys.argv[1], 'w').write(str(os.getpid())); time.sleep(10)")
            with self.assertRaises(TimeoutError):
                bench.run_one([sys.executable, "-c", child, str(pidfile)], os.environ.copy(), 0.7, 0.1, 0.02)
            self.assertTrue(pidfile.exists())
            self.assertFalse(bench.psutil.pid_exists(int(pidfile.read_text())))

    def test_output_cannot_overwrite_input(self):
        with tempfile.TemporaryDirectory(dir=TEST_ROOT) as directory:
            self.assertTrue(Path(directory).resolve().is_relative_to(TEST_ROOT))
            drawing = Path(directory) / "drawing with spaces.dxf"
            drawing.write_bytes(b"drawing stays intact")
            result = subprocess.run([sys.executable, str(BENCHMARK), "--drawing", str(drawing),
                                     "--executable", sys.executable, "--output", str(drawing)],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("must not overwrite", result.stderr)
            self.assertEqual(drawing.read_bytes(), b"drawing stays intact")

    def test_output_cannot_overwrite_input_through_hardlink(self):
        with tempfile.TemporaryDirectory(dir=TEST_ROOT) as directory:
            self.assertTrue(Path(directory).resolve().is_relative_to(TEST_ROOT))
            drawing = Path(directory) / "drawing.dxf"
            drawing.write_bytes(b"drawing stays intact")
            alias = Path(directory) / "alias.json"
            try:
                os.link(drawing, alias)
            except OSError as error:
                self.skipTest(f"Hardlinks unavailable: {error}")
            result = subprocess.run([sys.executable, str(BENCHMARK), "--drawing", str(drawing),
                                     "--executable", sys.executable, "--output", str(alias)],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("must not overwrite", result.stderr)
            self.assertEqual(drawing.read_bytes(), b"drawing stays intact")


if __name__ == "__main__":
    unittest.main()
