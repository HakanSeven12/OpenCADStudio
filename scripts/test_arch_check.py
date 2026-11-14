"""Tests for scripts/arch_check.py, run on small made-up source trees."""

import contextlib
import io
import os
from pathlib import Path
import re
import sys
import tempfile
import textwrap
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import arch_check  # noqa: E402


_TEMP = []


def tree(files):
    """Write {path under src/: text} to a temporary checkout and return it.
    The folders are removed when the tests finish."""
    temp = tempfile.TemporaryDirectory()
    _TEMP.append(temp)
    root = Path(temp.name)
    for rel, text in files.items():
        path = root / "src" / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(text), encoding="utf-8")
    return root


def edges(files):
    return arch_check.measure(tree(files))[0]


def run(argv, cwd, github_actions=False):
    """Run the tool in `cwd`; outside GitHub Actions unless asked, whatever
    the environment running the tests is."""
    env = {k: v for k, v in os.environ.items() if k != "GITHUB_ACTIONS"}
    if github_actions:
        env["GITHUB_ACTIONS"] = "true"
    out, old = io.StringIO(), os.getcwd()
    os.chdir(cwd)
    try:
        with mock.patch.dict(os.environ, env, clear=True), contextlib.redirect_stdout(out):
            code = arch_check.main(argv)
    finally:
        os.chdir(old)
    return code, out.getvalue()


def tearDownModule():
    for temp in _TEMP:
        temp.cleanup()


class Direction(unittest.TestCase):
    def test_upward_reference_is_reported_with_its_line(self):
        found = edges({"entities/a.rs": "fn f() {}\nfn g() { crate::scene::draw(); }\n"})
        self.assertEqual(found, {"entities -> scene": {"src/entities/a.rs": 2}})

    def test_downward_and_same_layer_references_are_not(self):
        found = edges({
            "scene/a.rs": "use crate::entities::Line;\n",
            "io/a.rs": "use crate::scene::Scene;\n",
            "app/a.rs": "use crate::cli::Args;\n",
        })
        self.assertEqual(found, {})

    def test_scene_must_not_use_io_although_same_layer(self):
        self.assertIn("scene -> io", edges({"scene/a.rs": "use crate::io::linetypes;\n"}))

    def test_grouped_use_counts_each_module(self):
        found = edges({"entities/a.rs": "use crate::{\n    snap::Hint,\n    scene::model::Object,\n};\n"})
        self.assertEqual(list(found), ["entities -> scene"])

    def test_a_file_counts_once_and_keeps_its_first_line(self):
        found = edges({"entities/a.rs": "\n\ncrate::scene::a();\ncrate::scene::b();\n"})
        self.assertEqual(found["entities -> scene"], {"src/entities/a.rs": 3})

    def test_crate_roots_and_binaries_are_skipped(self):
        found, unlayered = arch_check.measure(tree({"lib.rs": "use crate::app::Message;\n",
                                                    "bin/tool.rs": "use crate::app::Message;\n"}))
        self.assertEqual((found, unlayered), ({}, []))

    def test_modules_without_a_layer_are_listed(self):
        self.assertEqual(arch_check.measure(tree({"brand_new/a.rs": ""}))[1], ["brand_new"])


class IgnoredText(unittest.TestCase):
    def test_comments_and_strings(self):
        found = edges({"entities/a.rs": """\
            // crate::app::x
            /* crate::app::x /* nested crate::app::x */ crate::app::x */
            /// crate::app::x
            const A: &str = "crate::app::x";
            const B: &str = r#"say "crate::app::x" twice"#;
            const C: &[u8] = b"crate::app::x";
            const D: char = '"';
            fn f<'a>(s: &'a str) {}
            """})
        self.assertEqual(found, {})

    def test_line_numbers_survive_multiline_comments_and_strings(self):
        found = edges({"entities/a.rs": '/*\n\n*/\nconst S: &str = "\n\n";\ncrate::app::x();\n'})
        self.assertEqual(found["entities -> app"], {"src/entities/a.rs": 7})


class TestCode(unittest.TestCase):
    def test_inline_test_module_is_skipped(self):
        found = edges({"entities/a.rs": """\
            fn real() {}
            #[cfg(test)]
            mod tests {
                const BRACE: &str = "}";
                fn t() { crate::app::helper(); }
            }
            fn after() { crate::scene::draw(); }
            """})
        self.assertEqual(found, {"entities -> scene": {"src/entities/a.rs": 7}})

    def test_test_files_by_name_are_skipped(self):
        found = edges({
            "entities/a_tests.rs": "crate::app::x();\n",
            "entities/tests.rs": "crate::app::x();\n",
            "entities/tests/b.rs": "crate::app::x();\n",
            "entities/c_tests/d.rs": "crate::app::x();\n",
        })
        self.assertEqual(found, {})

    def test_out_of_line_test_module_and_its_children_are_skipped(self):
        found = edges({
            "entities/mod.rs": "#[cfg(test)]\nmod checks;\nmod real;\n",
            "entities/checks.rs": "mod more;\ncrate::app::x();\n",
            "entities/checks/more.rs": "crate::app::x();\n",
            "entities/real.rs": "crate::app::x();\n",
        })
        self.assertEqual(found, {"entities -> app": {"src/entities/real.rs": 1}})


class MessageException(unittest.TestCase):
    def test_ui_may_use_the_app_message_type(self):
        found = edges({
            "ui/a.rs": "use crate::app::Message;\n",
            "ui/b.rs": "fn f() -> crate::app::Message { crate::app::Message::Open }\n",
            "ui/c.rs": "use crate::app::{Message};\n",
            "ui/d.rs": "use crate::{app::Message, entities::Line};\n",
        })
        self.assertEqual(found, {})

    def test_anything_else_from_app_still_counts(self):
        found = edges({
            "ui/a.rs": "use crate::app::{Message, settings};\n",
            "ui/b.rs": "use crate::app::settings::Settings;\n",
            "ui/c.rs": "use crate::app;\n",
            "ui/d.rs": "use crate::{app::{Message, calc}};\n",
        })
        self.assertEqual(sorted(found["ui -> app"]),
                         ["src/ui/a.rs", "src/ui/b.rs", "src/ui/c.rs", "src/ui/d.rs"])

    def test_the_exception_is_for_ui_only(self):
        self.assertIn("modules -> app", edges({"modules/a.rs": "use crate::app::Message;\n"}))


class Diff(unittest.TestCase):
    def setUp(self):
        self.base = tree({"entities/a.rs": "crate::scene::x();\n",
                          "entities/b.rs": "crate::scene::x();\n"})
        self.head = tree({"entities/a.rs": "crate::scene::x();\n",
                          "entities/b.rs": "fn moved() {}\n",
                          "sys.rs": "fn f() {}\nfn g() { crate::io::bridge(); }\n"})

    def test_lists_added_and_removed_and_exits_1_when_something_was_added(self):
        code, out = run(["diff", str(self.base)], self.head)
        self.assertEqual(code, 1)
        self.assertIn("added    sys -> io", out)
        self.assertIn("src/sys.rs:2", out)
        self.assertIn("removed  entities -> scene", out)
        self.assertIn("src/entities/b.rs", out)
        self.assertNotIn("src/entities/a.rs", out)

    def test_exits_0_when_nothing_was_added(self):
        only_removes = tree({"entities/a.rs": "crate::scene::x();\n"})
        code, out = run(["diff", str(self.base)], only_removes)
        self.assertEqual(code, 0)
        self.assertIn("removed  entities -> scene", out)

    def test_markdown_marks_new_and_gone_pairs(self):
        _, out = run(["diff", str(self.base), "--markdown"], self.head)
        self.assertIn("`sys -> io` (new pair): `src/sys.rs:2`", out)
        _, out = run(["diff", str(self.head), "--markdown"], self.base)
        self.assertIn("`sys -> io` (pair now gone)", out)

    def test_annotations_only_inside_github_actions(self):
        _, out = run(["diff", str(self.base)], self.head, github_actions=True)
        self.assertIn("::warning file=src/sys.rs,line=2,title=Upward dependency::"
                      "sys (layer 0) uses io (layer 2)", out)
        _, out = run(["diff", str(self.base)], self.head)
        self.assertNotIn("::warning", out)
        _, out = run(["diff", str(self.base), "--markdown"], self.head, github_actions=True)
        self.assertNotIn("::warning", out)


class Report(unittest.TestCase):
    def test_report_counts_pairs_and_files(self):
        code, out = run(["report"], tree({"entities/a.rs": "crate::scene::x();\n",
                                          "entities/b.rs": "crate::scene::x();\n"}))
        self.assertEqual(code, 0)
        self.assertIn("Upward dependencies: 1 module pair, 2 files", out)


class RulesDocument(unittest.TestCase):
    def test_layer_table_matches_the_script(self):
        doc = (Path(__file__).resolve().parents[1] / "docs/architecture/RULES.md").read_text(encoding="utf-8")
        rows = re.findall(r"^\| (\d) [^|]*\| ([^|]*)\|", doc, re.M)
        documented = {name.strip("` "): int(layer) for layer, names in rows for name in names.split(",")}
        self.assertEqual(documented, arch_check.LAYERS)


if __name__ == "__main__":
    unittest.main()
