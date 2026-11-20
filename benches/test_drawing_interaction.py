"""Metric/validity tests; GUI and ETW are validated by the smoke recipe."""
import csv
import json
import os
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from drawing_interaction import Client, LOCAL_MEASUREMENTS, assert_state, distribution, frame_summaries, isolation_layout, local_output, model_space_owner, prepare_config, recovery_updates
from summarize_drawing_interaction import artifact_csv, summarize


class InteractionReportTests(unittest.TestCase):
    def test_edit_owner_comes_from_records_independent_of_capture_annotations(self):
        client = unittest.mock.Mock()
        client.call.return_value = {"records": [
            {"handle": "A", "name": "BLOCK_DEFINITION"},
            {"handle": "B", "name": "*MODEL_SPACE"}]}
        self.assertEqual(model_space_owner(client), "B")
        client.call.return_value = {"records": [{"handle": "A", "name": "BLOCK_DEFINITION"}]}
        with self.assertRaisesRegex(RuntimeError, "model-space block"):
            model_space_owner(client)

    def test_recovery_observations_ignore_temporary_and_unchanged_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "partial.tmp").write_bytes(b"unfinished")
            seen = {}
            self.assertEqual(recovery_updates(root, seen, 1), [])
            path = root / "drawing.sv$"
            path.write_bytes(b"complete")
            first = recovery_updates(root, seen, 2)
            self.assertEqual(len(first), 1)
            self.assertEqual(first[0]["observed_qpc"], 2)
            self.assertEqual(first[0]["bytes"], 8)
            self.assertEqual(recovery_updates(root, seen, 3), [])
            path.write_bytes(b"updated copy")
            self.assertEqual(recovery_updates(root, seen, 4)[0]["bytes"], 12)

    def test_state_request_control_has_no_drawing_operations(self):
        phases = []
        args = SimpleNamespace(scenario="state_requests", steps=4)
        client = unittest.mock.Mock()
        isolation_layout(client, args, "SINGLE", "SINGLE", "88", lambda *v: phases.append(v))
        client.call.assert_not_called()
        self.assertEqual(phases, [("state_requests", "SINGLE", [("state", {})] * 4)])

    def test_idle_control_does_not_recreate_viewports_or_edit(self):
        phases = []
        args = SimpleNamespace(scenario="idle", cycle_seconds=5)
        client = unittest.mock.Mock()
        active = isolation_layout(client, args, "SINGLE", "SINGLE", "88", lambda *v: phases.append(v))
        self.assertEqual(active, "SINGLE")
        client.call.assert_not_called()
        self.assertEqual(phases, [("idle", "SINGLE", [], 5)])

    def test_fixed_edit_control_keeps_panes_and_returns_geometry_with_inverse_moves(self):
        phases = []
        args = SimpleNamespace(scenario="edits", edit_steps=3)
        client = unittest.mock.Mock()
        isolation_layout(client, args, "SINGLE", "SINGLE", "88", lambda *v: phases.append(v))
        client.call.assert_not_called()
        moves = [fields["cmd"] for op, fields in phases[0][2] if op == "commands"]
        self.assertEqual(moves, ["MOVE 0,0,0 1,0,0", "MOVE 0,0,0 -1,0,0"] * 3)
        self.assertFalse(any(op == "undo" for op, _ in phases[0][2]))

    def test_layout_control_recreates_even_when_name_matches_and_never_moves_entities(self):
        phases = []
        args = SimpleNamespace(scenario="layout_churn", layout_settle_seconds=.25)
        client = unittest.mock.Mock()
        client.call.return_value = {"entries": [{"kind": "Output", "text": "VPORTS: 1 viewport(s)."}]}
        isolation_layout(client, args, "SINGLE", "SINGLE", "88", lambda *v: phases.append(v))
        self.assertEqual(phases, [("layout", "SINGLE", [("commands", {"cmd": "VPORTS SINGLE"})], .25)])
        self.assertEqual(client.call.call_args_list,
                         [unittest.mock.call("history"), unittest.mock.call("commands", cmd="ZOOM EXTENTS")])

    def test_edit_layout_fits_every_pane_before_timing_edits(self):
        phases = []
        args = SimpleNamespace(scenario="edits", layout_settle_seconds=.25, edit_steps=1)
        client = unittest.mock.Mock()
        client.call.side_effect = [
            {"entries": [{"kind": "Output", "text": "VPORTS: 4 viewport(s)."}]},
            {"viewport_size": [800, 600]}]
        isolation_layout(client, args, "4", "SINGLE", "88", lambda *v: phases.append(v))
        self.assertEqual([p[0] for p in phases], ["layout", "fit_panes", "move_retained"])
        events = phases[1][2]
        centers = [fields["events"][0]["move"] for op, fields in events if op == "pointer"]
        self.assertEqual(centers, [[200, 150], [600, 150], [200, 450], [600, 450], [200, 150]])
        self.assertEqual(sum(op == "commands" and fields["cmd"] == "ZOOM EXTENTS"
                             for op, fields in events), 5)

    def test_measurement_outputs_are_confined_to_ignored_folder(self):
        allowed = LOCAL_MEASUREMENTS / "example" / "run.json"
        self.assertEqual(local_output(allowed), allowed.resolve())
        for path in (LOCAL_MEASUREMENTS, LOCAL_MEASUREMENTS.parent / "results" / "run.json",
                     LOCAL_MEASUREMENTS / ".." / "run.json"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                local_output(path)

    def test_moved_report_finds_adjacent_csv_without_rewriting_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "run.json"
            csv = report.with_suffix(".artifacts") / "drawing-1-run-1" / "frames.csv"
            csv.parent.mkdir(parents=True)
            csv.write_text("header\n")
            self.assertEqual(artifact_csv(report, r"C:\old\run.artifacts\drawing-1-run-1\frames.csv"), csv)
            with self.assertRaises(FileNotFoundError):
                artifact_csv(report, r"C:\old\run.artifacts\..\frames.csv")

    def test_custom_recovery_path_is_refused_before_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / "source" / "OpenCADStudio"
            config.mkdir(parents=True)
            (config / "settings.json").write_text(json.dumps({"settings": {"save_file_path": "C:/user/recovery"}}))
            with patch.dict(os.environ, {"APPDATA": str(config.parent)}, clear=True):
                with self.assertRaisesRegex(RuntimeError, "bypass config isolation"):
                    prepare_config(root / "artifacts")

    def test_blank_recovery_path_keeps_original_settings_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / "source" / "OpenCADStudio"
            config.mkdir(parents=True)
            original = b'{"settings": {"save_file_path": "", "savetime_min": 10}}\n'
            (config / "settings.json").write_bytes(original)
            with patch.dict(os.environ, {"APPDATA": str(config.parent)}, clear=True):
                copied, hashes = prepare_config(root / "artifacts")
            self.assertEqual((copied / "settings.json").read_bytes(), original)
            self.assertIn("settings.json", hashes)

    def fake_client(self, responses):
        client = object.__new__(Client)
        client.document_id = 42
        client.request_prefix = "test"
        client.serial = 0
        client.timeout = 1
        client.pace_click = lambda *_: None
        calls = []

        def raw(op, fields):
            calls.append((op, fields.copy()))
            return responses.pop(0)

        client.raw = raw
        return client, calls

    def test_async_mutation_is_polled_by_id_and_never_replayed(self):
        client, calls = self.fake_client([
            {"ok": True, "status": "accepted"}, {"ok": True, "status": "running"},
            {"ok": True, "status": "completed", "result": {"done": True}}])
        with patch("drawing_interaction.time.sleep"):
            result = client.call("commands", cmd="MOVE 0,0 1,0")
        self.assertEqual(result["result"], {"done": True})
        self.assertEqual([op for op, _ in calls], ["commands", "operation", "operation"])
        self.assertEqual(calls[0][1]["document_id"], 42)
        self.assertTrue(all(fields["request_id"] == "test-1" for _, fields in calls))

    def test_interactive_prompt_is_rejected_without_supplying_more_input(self):
        client, calls = self.fake_client([{"ok": True, "status": "waiting_input"}])
        with self.assertRaisesRegex(RuntimeError, "did not complete"):
            client.call("commands", cmd="MOVE")
        self.assertEqual(len(calls), 1)

    def test_distribution_uses_nearest_rank_and_preserves_tail(self):
        self.assertIsNone(distribution([]))
        self.assertEqual(distribution([100, 1, 2, 3]),
                         {"count": 4, "median": 2.5, "p95": 100, "max": 100})

    def test_frame_intervals_do_not_cross_phases_processes_or_swap_chains(self):
        fields = ["ProcessID", "SwapChainAddress", "QPCTime", "msBetweenPresents", "Dropped", "msUntilDisplayed"]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "frames.csv"
            with path.open("w", newline="") as stream:
                writer = csv.writer(stream)
                writer.writerow(fields)
                writer.writerows([
                    [7, "a", 50, 500, 0, 4],     # prior phase / idle
                    [7, "a", 110, 600, 0, 3],    # first in phase, omit interval
                    [7, "b", 115, 700, 0, 2],    # another swap chain
                    [99, "a", 120, 900, 0, 1],   # another process
                    [7, "a", 130, 20, 1, 0],     # dropped, still a present
                    [7, "a", 150, 20, 0, 5],
                    [7, "b", 160, 45, 0, 2],
                    [7, "a", 210, 60, 0, 6],    # first in following phase
                ])
            phases = [{"start_qpc": 100, "end_qpc": 200},
                      {"start_qpc": 200, "end_qpc": 300}, {"start_qpc": 301}]
            summary = frame_summaries(path, phases, 7)
        self.assertEqual(summary, {"rows_for_pid": 7, "swap_chains": 2})
        a, b = phases[0]["presentmon"]
        self.assertEqual(a["between_presents_ms"]["median"], 20)
        self.assertEqual(a["between_presents_ms"]["count"], 2)
        self.assertEqual(a["displayed"], 2)
        self.assertEqual(a["dropped"], 1)
        self.assertEqual(a["present_to_display_ms"]["median"], 4)
        self.assertEqual(b["between_presents_ms"]["median"], 45)
        self.assertIsNone(phases[1]["presentmon"][0]["between_presents_ms"])
        self.assertNotIn("presentmon", phases[2])

    def test_unknown_frame_schema_is_an_error_not_zero_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "frames.csv"
            path.write_text("ProcessID,FrameTime\n7,16\n")
            with self.assertRaisesRegex(RuntimeError, "Unsupported PresentMon"):
                frame_summaries(path, [], 7)

    def test_perf_modal_and_editors_invalidate_the_workload(self):
        good = {"mode": "gui", "read_only": True, "tool_windows": []}
        assert_state(good)
        for change in ({"read_only": False}, {"mode": "headless"},
                       {"tool_windows": ["Perf"]}, {"modal": "Notice"},
                       {"command": {"name": "MOVE"}}, {"text_editor": True}):
            with self.subTest(change=change), self.assertRaises(RuntimeError):
                assert_state(good | change)

    def test_summary_omits_private_paths_settings_and_geometry(self):
        run = dict.fromkeys(("drawing_sha256", "live_version", "transport", "viewport_size", "gpu_adapter",
                  "settings_sha256", "selection_click_min_interval_ms", "model_line_count", "cycles", "active_seconds",
                  "all_phase_boundaries_foreground", "termination"))
        run.update(valid=True, perf_log_lines=0, memory_sampler_error=None,
                   baseline={"qpc": 100, "rss_bytes": 1}, final={"qpc": 110, "rss_bytes": 2},
                   memory_samples=[{"rss_bytes": 2}], checkpoints=[], edit_handle="PRIVATE_GEOMETRY",
                   final_state={"documents": [{"path": "PRIVATE_DRAWING"}]},
                   dirty_at_start=True,
                   recovery_events=[{"observed_qpc": 120, "bytes": 1234, "name": "PRIVATE_RECOVERY"}],
                   frame_capture={"available": False, "csv": "PRIVATE_CSV"},
                   phases=[{"layout": "SINGLE", "name": "pan", "requests": [
                       {"op": "pointer", "start_qpc": 105, "roundtrip_ms": 20},
                       {"op": "pointer", "start_qpc": 106, "roundtrip_ms": 30}],
                       "all_polls_foreground": True}])
        report = {"schema": 1, "benchmark": "drawing_interaction", "sources_unchanged": True,
                  "harness_sha256": "hash", "timestamp_utc": "time", "settings": {},
                  "system": {"qpc_frequency": 10}, "runs": [run],
                  "executable": {"version": "2026.46", "sha256": "hash", "path": "PRIVATE_EXE"},
                  "drawings": [{"bytes": 123, "sha256": "hash", "path": "PRIVATE_DRAWING"}]}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            path.write_text(json.dumps(report))
            summary = summarize(path)
            self.assertNotIn("PRIVATE", json.dumps(summary))
            self.assertEqual(summary["runs"][0]["recovery_updates"], [{"elapsed_seconds": 2, "bytes": 1234}])
            self.assertTrue(summary["runs"][0]["dirty_at_start"])
            self.assertEqual([r["elapsed_seconds"] for r in summary["runs"][0]["slowest_requests"]], [.6, .5])
            report["sources_unchanged"] = False
            path.write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError, "stability"):
                summarize(path)


if __name__ == "__main__":
    unittest.main()
