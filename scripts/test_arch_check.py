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


class TestOnlyCfg(unittest.TestCase):
    def test_all_with_test_is_test_code(self):
        found = edges({"entities/a.rs": '#[cfg(all(test, not(target_arch = "wasm32")))]\nmod tests {\n    use crate::scene::S;\n}\n'})
        self.assertEqual(found, {})

    def test_other_cfgs_are_production_code(self):
        found = edges({"entities/a.rs": '#[cfg(any(test, feature = "x"))]\nmod m {\n    use crate::scene::S;\n}\n'
                                        '#[cfg(all(not(test), unix))]\nmod n {\n    use crate::scene::T;\n}\n'})
        self.assertEqual(found, {"entities -> scene": {"src/entities/a.rs": 3}})

    def test_predicates(self):
        self.assertTrue(arch_check.test_only("test"))
        self.assertTrue(arch_check.test_only('all(test, not(target_arch = "wasm32"))'))
        self.assertFalse(arch_check.test_only("all(not(test), unix)"))
        self.assertFalse(arch_check.test_only("any(test, unix)"))


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
        found = edges({"entities/a.rs": "use crate::{\n    parametric::Hint,\n    scene::model::Object,\n};\n"})
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


def ready(files):
    """{crate: (uses, test_uses)} for the crates with a finding."""
    result = arch_check.readiness(tree(files))
    return {c["crate"]: (c["uses"], c["test_uses"]) for c in result["crates"] if c["uses"] or c["test_uses"]}


class Readiness(unittest.TestCase):
    def test_a_crate_may_use_its_own_crate_and_the_crates_below(self):
        self.assertEqual(ready({"entities/a.rs": "use crate::config::C;\nuse crate::text::T;\n",
                                "scene/a.rs": "use crate::entities::E;\nuse crate::snap::S;\n"}), {})

    def test_a_same_layer_crate_blocks_although_the_layer_rule_allows_it(self):
        self.assertEqual(ready({"io/a.rs": "fn f() {}\nuse crate::scene::S;\n"}),
                         {"ocs_io": ({"io -> scene": {"src/io/a.rs": 2}}, {})})

    def test_the_message_exception_does_not_apply_to_a_crate(self):
        self.assertEqual(edges({"ui/a.rs": "use crate::app::Message;\n"}), {})
        self.assertEqual(ready({"ui/a.rs": "use crate::app::Message;\n"}),
                         {"ocs_ui": ({"ui -> app": {"src/ui/a.rs": 1}}, {})})

    def test_a_module_in_no_planned_crate_blocks_and_is_listed(self):
        self.assertEqual(ready({"io/a.rs": "use crate::catalog::Linetype;\n"}),
                         {"ocs_io": ({"io -> catalog": {"src/io/a.rs": 1}}, {})})
        self.assertIn("catalog", arch_check.readiness(tree({}))["unplaced"])

    def test_rendering_may_use_the_session_crate(self):
        self.assertTrue(arch_check.may_use("ocs_render", "ocs_session"))
        self.assertFalse(arch_check.may_use("ocs_session", "ocs_render"))
        self.assertFalse(arch_check.may_use("ocs_io", "ocs_render"))
        self.assertFalse(arch_check.may_use("ocs_base", None))

    def test_test_code_is_counted_apart(self):
        found = ready({"entities/a.rs": "fn f() {}\n#[cfg(test)]\nmod tests {\n    use crate::scene::S;\n}\n",
                       "entities/b_tests.rs": "use crate::io::R;\n"})
        self.assertEqual(found, {"ocs_model": ({}, {"entities -> scene": {"src/entities/a.rs": 4},
                                                    "entities -> io": {"src/entities/b_tests.rs": 1}})})


class ReadinessRootPaths(unittest.TestCase):
    LIB = ("pub mod app;\npub mod ui;\npub(crate) use app::QuadOptionMsg;\n"
           "#[cfg(test)]\npub(crate) fn with_headless_ui() {\n    let x = 1;\n}\n")

    def test_a_re_export_in_the_crate_root_is_a_use_of_its_module(self):
        found = ready({"lib.rs": self.LIB, "ui/a.rs": "use crate::{QuadOptionMsg, i18n};\n"})
        self.assertEqual(found, {"ocs_ui": ({"ui -> app": {"src/ui/a.rs": 1}}, {})})

    def test_an_item_of_the_crate_root_blocks(self):
        found = ready({"lib.rs": self.LIB, "ui/a.rs": "#[cfg(test)]\nmod tests {\n    fn t() { crate::with_headless_ui(); }\n}\n"})
        self.assertEqual(found, {"ocs_ui": ({}, {"ui -> crate root": {"src/ui/a.rs": 3}})})

    def test_super_paths_that_leave_the_top_module(self):
        found = ready({
            "entities/mod.rs": "use super::scene::A;\nmod inner {\n    use super::super::io::B;\n    use super::C;\n}\n",
            "entities/deep/x.rs": "use super::super::super::scene::D;\nuse super::super::shape::E;\n",
            "entities/y.rs": "use super::{z::F, super::ui::G};\n",
        })
        self.assertEqual(found, {"ocs_model": ({
            "entities -> scene": {"src/entities/deep/x.rs": 1, "src/entities/mod.rs": 1},
            "entities -> io": {"src/entities/mod.rs": 3},
        }, {})})

    def test_super_inside_the_module_and_test_globs_are_not_uses(self):
        self.assertEqual(ready({"entities.rs": "fn f() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n",
                                "scene/a.rs": "use super::b::X;\n"}), {})

    def test_a_glob_of_the_crate_root_blocks(self):
        self.assertEqual(ready({"entities.rs": "use super::*;\n"}),
                         {"ocs_model": ({"entities -> crate root": {"src/entities.rs": 1}}, {})})


class ReadinessMacros(unittest.TestCase):
    def test_an_exported_macro_is_a_use_of_the_module_that_defines_it(self):
        files = {"i18n.rs": "#[macro_export]\nmacro_rules! t {\n    ($s:expr) => { $crate::i18n::translate($s) };\n}\n",
                 "scene/m.rs": "#[macro_export]\nmacro_rules! draw {\n    () => { $crate::scene::draw() };\n}\n",
                 "entities/a.rs": "fn f() { crate::t!(\"x\"); }\nfn g() { crate::draw!(); }\n"}
        self.assertEqual(ready(files), {"ocs_model": ({"entities -> scene": {"src/entities/a.rs": 2}}, {})})

    def test_a_plain_crate_path_in_an_exported_macro_counts_at_each_caller(self):
        # `$crate::io` in `n` names the crate of i18n: only i18n uses io.
        files = {"i18n.rs": "#[macro_export]\nmacro_rules! m {\n    () => { crate::scene::draw() };\n}\n"
                            "#[macro_export]\nmacro_rules! n {\n    () => { $crate::io::read() };\n}\n",
                 "entities/a.rs": "use crate::m;\nfn f() { m!(); }\n",
                 "entities/b.rs": "fn g() { crate::n!(); }\n"}
        self.assertEqual(ready(files), {
            "ocs_base": ({"i18n -> io": {"src/i18n.rs": 7}, "i18n -> scene": {"src/i18n.rs": 3}}, {}),
            "ocs_model": ({"entities -> scene": {"src/entities/a.rs": 2}}, {}),
        })


class ReadinessCommand(unittest.TestCase):
    def test_text_and_markdown(self):
        root = tree({"io/a.rs": "use crate::scene::S;\n"})
        code, out = run(["readiness"], root)
        self.assertEqual(code, 0)
        self.assertRegex(out, r"ocs_base\s+production: ready\s+tests: ready")
        self.assertRegex(out, r"ocs_io\s+production: 1 file\s+tests: ready")
        self.assertRegex(out, r"io -> scene\s+1")
        self.assertIn("Modules in no planned crate (see CRATES): catalog", out)
        code, out = run(["readiness", "--markdown"], root)
        self.assertIn("| `ocs_session` | — | no modules yet | no modules yet |", out)
        self.assertIn("- `io -> scene`: 1 file, first `src/io/a.rs:1`", out)


class TargetDocument(unittest.TestCase):
    def test_crates_match_the_target_and_the_layers(self):
        doc = (Path(__file__).resolve().parents[1] / "docs/architecture/TARGET.md").read_text(encoding="utf-8")
        rows = dict(re.findall(r"^\| `(\w+)`[^|]*\| [^|]*\| (`[^|]*)\|", doc, re.M))
        crates = {name: mods for name, _, mods in arch_check.CRATES}
        for name, cell in rows.items():
            if name in crates:
                named = re.findall(r"`(\w+)`", cell.split(":")[0])
                self.assertLessEqual(set(named), set(crates[name]), name)
        self.assertIn("ocs_base", rows)
        placed = [m for mods in crates.values() for m in mods]
        self.assertEqual(len(placed), len(set(placed)), "a module in two crates")
        self.assertLessEqual(set(placed), set(arch_check.LAYERS))
        for name, layer, mods in arch_check.CRATES:
            self.assertIn(f"`{name}`", doc)
            layers = {3, 4} if name == "ocs_commands" else {layer}
            for m in mods:
                if m != "entity_creation":  # layer 4, but TARGET.md puts it in the model crate
                    self.assertIn(arch_check.LAYERS[m], layers, m)


class RulesDocument(unittest.TestCase):
    def test_layer_table_matches_the_script(self):
        doc = (Path(__file__).resolve().parents[1] / "docs/architecture/RULES.md").read_text(encoding="utf-8")
        rows = re.findall(r"^\| (\d) [^|]*\| ([^|]*)\|", doc, re.M)
        documented = {name.strip("` "): int(layer) for layer, names in rows for name in names.split(",")}
        self.assertEqual(documented, arch_check.LAYERS)


if __name__ == "__main__":
    unittest.main()
