#!/usr/bin/env python3
"""Render the Code Health wiki page.

    python3 tools/wiki/code_health.py WIKI_DIR HEALTH_JSON [--credits FILE] [--release TAG]

HEALTH_JSON is `scripts/health.py measure --cargo` output for the release.
The page's history lives in the wiki itself (WIKI_DIR/health/history.json):
this run appends the release to it, so the trend needs no commits to main.
Goals come from docs/architecture/health-goals.json. Writes
WIKI_DIR/Code-Health.md.
"""
import importlib.util
import json
import math
import os
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
GOALS = ROOT / "docs" / "architecture" / "health-goals.json"
REPO = os.environ.get("GITHUB_REPOSITORY", "HakanSeven12/OpenCADStudio")
BLOB = f"https://github.com/{REPO}/blob"
SPARK = "▁▂▃▄▅▆▇█"

_spec = importlib.util.spec_from_file_location("health_tool", ROOT / "scripts" / "health.py")
health_tool = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(health_tool)
LABELS = health_tool.LABELS


def fmt(v):
    if v is None:
        return "—"
    return f"{v:,.1f}" if isinstance(v, float) else f"{v:,}"


def milestone(goal, baseline, now):
    """(start, milestone) for the stretch now is in. Counts halve from the
    baseline toward the target; rising measures step up by 10% (coverage by
    5 points). None when there is no baseline or the target is reached."""
    if baseline is None or now is None:
        return None
    target = goal.get("target")
    if goal["direction"] == "down":
        floor = target if target is not None else 0
        start, m = baseline, baseline / 2
        while now <= m and m > floor:
            start, m = m, m / 2
        m = max(math.floor(m), floor)
        if now <= floor:
            return None
        return start, m
    step = (lambda x: x + 5) if goal["id"].startswith("coverage") else (lambda x: x * 1.1)
    start, m = baseline, step(baseline)
    while now >= m:
        start, m = m, step(m)
    cap = 100 if goal["id"].startswith("coverage") else None
    return start, (min(m, cap) if cap else math.ceil(m))


def progress(goal, baseline, now):
    if goal.get("milestones") is False:
        # A measure to watch, not to drive down or up: only flag drift.
        if baseline is None or now is None:
            return ""
        drifted = now > baseline if goal["direction"] == "down" else now < baseline
        return (f"{'above' if goal['direction'] == 'down' else 'below'} the baseline ({fmt(baseline)})"
                if drifted else f"keep {'at or below' if goal['direction'] == 'down' else 'at or above'} {fmt(baseline)}")
    ms = milestone(goal, baseline, now)
    if ms is None:
        if baseline is not None and now is not None and goal.get("target") is not None and now <= goal["target"]:
            return "reached the target"
        return ""
    start, m = ms
    span = (start - m) if goal["direction"] == "down" else (m - start)
    done = ((start - now) if goal["direction"] == "down" else (now - start)) / span if span else 0
    worse = (now > baseline) if goal["direction"] == "down" else (now < baseline)
    if worse:
        return f"worse than the baseline ({fmt(baseline)})"
    done = max(0.0, min(1.0, done))
    bars = round(done * 10)
    return f"`{'▰' * bars}{'▱' * (10 - bars)}` {round(done * 100)}% to {fmt(m)}"


def spark(values):
    vals = [v for v in values if v is not None]
    if len(vals) < 2:
        return ""
    lo, hi = min(vals), max(vals)
    return "".join(SPARK[0] if hi == lo else SPARK[round((v - lo) / (hi - lo) * 7)] for v in vals)


def change(goal, before, now):
    if before is None or now is None or before == now:
        return "no change" if before == now and now is not None else "—"
    better = (now < before) == (goal["direction"] == "down")
    return f"{'better' if better else 'worse'} ({'+' if now > before else ''}{fmt(now - before)})"


def link(path, commit, line=None):
    url = f"{BLOB}/{commit}/{path}" + (f"#L{line}" if line else "")
    return f"[`{path}{':' + str(line) if line else ''}`]({url})"


LAYER_NAMES = {0: "Foundations", 1: "Drawing model and geometry", 2: "File formats and rendering",
               3: "Tool contract and plugin runtime", 4: "Features (modules)"}


def ranked_test_targets(cur):
    """Core files ranked by uncovered lines weighted by how often they
    changed in the last 28 days: (score, file, uncovered, lines, landings)."""
    d = cur["details"]
    churn = d.get("churn_28d", {})
    out = []
    for f, c in d.get("coverage_files", {}).items():
        unc = c["lines"] - c["covered"]
        if unc:
            out.append((unc * (1 + churn.get(f, 0)), f, unc, c["lines"], churn.get(f, 0)))
    return sorted(out, key=lambda t: (-t[0], t[1]))


def test_tasks(cur, commit):
    d = cur["details"]
    files = d.get("coverage_files")
    if not files:
        return []
    out = ["### Add tests here", "",
           "Core-layer files with the most lines no test runs, weighted by how often the file changed in the "
           "last 28 days, so a test here protects code that is actively changing. *Size: one test or more.*", "",
           "| File | Lines not run by tests | Coverage | Changes in 28 days |", "| --- | ---: | ---: | ---: |"]
    for _, f, unc, n, landings in ranked_test_targets(cur)[:10]:
        out.append(f"| {link(f, commit)} | {unc:,} | {100.0 * (n - unc) / n:.1f}% | {landings} |")
    out.append("")
    untested = d.get("untested_files", [])
    if untested:
        biggest = sorted(untested, key=lambda f: -files[f]["lines"])
        out += [f"**No tests yet:** {len(untested)} core-layer {'file is' if len(untested) == 1 else 'files are'} "
                "not run by any test. The largest:", ""]
        out += [f"- {link(f, commit)}: {files[f]['lines']:,} lines" for f in biggest[:8]]
        out.append("")
    return out


def coverage_by_layer(cur, prev, history):
    v = cur["values"]
    keys = sorted(k for k in v if k.startswith("coverage_layer_"))
    if not keys:
        return []
    files = cur["details"].get("coverage_files", {})
    out = ["### Coverage by layer", "",
           "Line coverage of production code in each core layer (test code, generated files and the "
           f"[excluded paths]({BLOB}/main/docs/architecture/health-goals.json) are left out).", "",
           "| Layer | Coverage | Lines not run by tests | Since last release | Trend |",
           "| --- | ---: | ---: | --- | --- |"]
    up = {"id": "coverage_layer", "direction": "up"}
    for k in keys:
        layer = int(k.rsplit("_", 1)[1])
        unc = sum(f["lines"] - f["covered"] for f in files.values() if f["layer"] == layer)
        before = prev["values"].get(k) if prev else None
        trend = spark([h["values"].get(k) for h in history[-12:]])
        shown = f"{fmt(v[k])}%" if v[k] is not None else "—"
        out.append(f"| {layer} {LAYER_NAMES.get(layer, '')} | {shown} | {unc:,} | {change(up, before, v[k])} | {trend} |")
    return out + [""]


def cleanup_tasks(cur, commit):
    d = cur["details"]
    out = []
    small = []
    for pair, files in d.get("upward_items", {}).items():
        if len(files) == 1:
            (f, e), = files.items()
            if len(e["items"]) <= 2 and "(module)" not in e["items"]:
                dst = pair.split(' -> ')[1]
                items = ", ".join(f"`{dst}::{i}`" for i in e["items"])
                small.append(f"- Move {items} to a lower layer (or pass it in), used at {link(f, commit, e['line'])}. "
                             f"Removes the `{pair}` dependency. *Size: one file.*")
    if small:
        out += ["### Remove a whole upward dependency", "",
                "Each of these is the only use of a higher module by a lower one. Moving the item to a lower "
                "module (or passing it in) removes the dependency. Pure moves are easiest to review.", ""] + small + [""]
    io_panics = [x for x in d.get("panic_sites", []) if x["file"].startswith("src/io/")]
    if io_panics:
        out += ["### Replace a panic in file import or export", "",
                f"{len(io_panics)} {'place' if len(io_panics) == 1 else 'places'} in `src/io` "
                f"{'stops' if len(io_panics) == 1 else 'stop'} the app if "
                "their assumption fails. Where the value comes "
                "from a file, return an error instead and add a test with a file that triggers it. "
                "*Size: a few lines each.*", ""]
        out += [f"- {link(x['file'], commit, x['line'])}" for x in io_panics[:10]]
        if len(io_panics) > 10:
            out.append(f"- …and {len(io_panics) - 10} more (`python3 scripts/health.py measure` lists them all)")
        out.append("")
    unsafe = d.get("unsafe_undocumented", [])
    if unsafe:
        by_file = {}
        for x in unsafe:
            by_file.setdefault(x["file"], []).append(x["line"])
        out += ["### Document an unsafe block", "",
                "Add a `// SAFETY:` comment saying why the block is sound (for an `unsafe fn`, a `# Safety` doc section). "
                "*Size: comments only.*", ""]
        for f, lines in sorted(by_file.items(), key=lambda kv: -len(kv[1]))[:8]:
            out.append(f"- {link(f, commit)}: {len(lines)} (first at line {lines[0]})")
        out.append("")
    fixable = d.get("clippy_fixable_by_lint")
    if fixable:
        out += ["### Apply an automatic clippy fix", "",
                "These warnings have a fix clippy can apply. One lint per pull request keeps the diff easy to review: "
                "`cargo clippy --fix --workspace --all-targets -- -A clippy::all -W <lint>`, then check the diff. "
                "*Size: mechanical.*", "", "| Lint | Fixable warnings |", "| --- | ---: |"]
        out += [f"| `{code}` | {n:,} |" for code, n in fixable[:10]] + [""]
    out += test_tasks(cur, commit)
    long_fns = d.get("long_functions", [])
    if long_fns:
        out += ["### Discuss splitting a long function", "",
                "Larger work: propose a split in a discussion first.", ""]
        out += [f"- `{x['name']}` in {link(x['file'], commit, x['line'])}: {x['lines']:,} lines" for x in long_fns[:5]]
        out.append("")
    return out


def thanks(credits):
    """One line per contributor, in alphabetical order (never a ranking):
    what their merged work improved in this release, summed per measure, and
    the pull requests it came from."""
    if credits is None:
        return ["The thanks list was not computed for this release.", ""]
    if not credits:
        return ["No merged pull request improved a measure in this release. Pick a task below to be the first.", ""]
    # A merge's subject may carry a login and another merge by the same
    # person may not: give every entry the login seen for its author's name,
    # so each contributor gets one line.
    login_of = {c["author"].lower(): c["login"] for c in credits if c.get("login") and c.get("author")}
    people = {}
    for c in credits:
        c = dict(c, login=c.get("login") or login_of.get((c.get("author") or "").lower()))
        name = c.get("login") or c.get("author") or "unknown"
        person = people.setdefault(name.lower(), {"name": name, "login": c.get("login"), "totals": {}, "prs": [], "direct": 0})
        person["login"] = person["login"] or c.get("login")
        for k, (a, b) in c["improved"].items():
            person["totals"][k] = person["totals"].get(k, 0) + (b - a)
        if c.get("pr"):
            if c["pr"] not in person["prs"]:
                person["prs"].append(c["pr"])
        else:
            person["direct"] += 1
    out = []
    for key in sorted(people):
        person = people[key]
        who = f"[{person['name']}](https://github.com/{person['login']})" if person["login"] else person["name"]
        what = "; ".join(f"{LABELS.get(k, k)} {'+' if d > 0 else '−'}{fmt(abs(d))}" for k, d in person["totals"].items())
        where = [f"[#{n}](https://github.com/{REPO}/pull/{n})" for n in person["prs"][:8]]
        if len(person["prs"]) > 8:
            where.append(f"and {len(person['prs']) - 8} more")
        if person["direct"]:
            where.append(f"{person['direct']} direct commit{'' if person['direct'] == 1 else 's'}")
        out.append(f"- {who}: {what} ({', '.join(where)})")
    return out + [""]


def render(cur, goals, history, credits, release):
    commit = cur["commit"]
    prev = history[-2] if len(history) > 1 else None
    first = {g["id"]: next((h["values"].get(g["id"]) for h in history if h["values"].get(g["id"]) is not None), None)
             for g in goals["measures"]}
    lines = ["# Code Health", "",
             f"Measured at {release} (`{commit[:9]}`), {date.today().isoformat()}. Rebuilt on every release from the code; "
             "nothing here is edited by hand.", "",
             "**How to help:** pick a task under *Pick up a cleanup*; check the *Code health* section in your pull "
             "request's checks summary, which shows what your change improves; read "
             f"[how this is measured]({BLOB}/main/docs/architecture/HEALTH.md).", ""]
    lines += ["## Scorecard", ""]
    for area in dict.fromkeys(g["area"] for g in goals["measures"]):
        gs = [g for g in goals["measures"] if g["area"] == area]
        improved = sum(1 for g in gs if prev and change(g, prev["values"].get(g["id"]), cur["values"].get(g["id"])).startswith("better"))
        lines += [f"### {area}", ""]
        if prev:
            lines += [f"{improved} of {len(gs)} improved since {prev['release']}.", ""]
        lines += ["| Measure | Now | Since last release | Next milestone | Trend | Long-term |",
                  "| --- | ---: | --- | --- | --- | ---: |"]
        for g in gs:
            now = cur["values"].get(g["id"])
            base = g["baseline"] if g["baseline"] is not None else first[g["id"]]
            before = prev["values"].get(g["id"]) if prev else None
            trend = spark([h["values"].get(g["id"]) for h in history[-12:]])
            lines.append(f"| [{g['label']}]({g['learn'] if g['learn'].startswith('http') else BLOB + '/main/' + g['learn']}) "
                         f"| {fmt(now)} | {change(g, before, now)} | {progress(g, base, now)} | {trend} "
                         f"| {fmt(g['target']) if g['target'] is not None else ('lower' if g['direction'] == 'down' else 'higher')} |")
        lines.append("")
    lines += coverage_by_layer(cur, prev, history)
    lines += ["## Thanks", "", "Contributors whose merged work improved a measure in this release, in alphabetical order:", ""] + thanks(credits)
    lines += ["## Pick up a cleanup", ""] + cleanup_tasks(cur, commit)
    recent = history[-6:][::-1]
    lines += ["## History", "", "| Measure | " + " | ".join(h["release"] for h in recent) + " |",
              "| --- |" + " ---: |" * len(recent)]
    for g in goals["measures"]:
        lines.append(f"| {g['label']} | " + " | ".join(fmt(h["values"].get(g["id"])) for h in recent) + " |")
    lines += ["", "## Why these measures", ""]
    lines += [f"- **{g['label']}**: {g['why']}" for g in goals["measures"]]
    return "\n".join(lines) + "\n"


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    wiki, health_file = Path(argv[0]), Path(argv[1])
    credits_file = Path(argv[argv.index("--credits") + 1]) if "--credits" in argv else None
    release = argv[argv.index("--release") + 1] if "--release" in argv else "main"
    cur = json.loads(health_file.read_text())
    goals = json.loads(GOALS.read_text())
    hist_file = wiki / "health" / "history.json"
    history = json.loads(hist_file.read_text()) if hist_file.exists() else []
    history = [h for h in history if h["release"] != release]
    history.append({"release": release, "commit": cur["commit"], "date": date.today().isoformat(), "values": cur["values"]})
    # None when the replay did not finish: the page then says so, instead of
    # claiming that nothing improved.
    try:
        credits = json.loads(credits_file.read_text()) if credits_file else None
    except (OSError, ValueError):
        credits = None
    hist_file.parent.mkdir(parents=True, exist_ok=True)
    hist_file.write_text(json.dumps(history, indent=1) + "\n")
    (wiki / "Code-Health.md").write_text(render(cur, goals, history, credits, release))
    # For the pull request job's "no tests yet" note: kept only when this
    # release measured coverage, so a failed coverage run keeps the last list.
    if cur["details"].get("coverage_files"):
        (wiki / "health" / "untested.json").write_text(json.dumps(cur["details"]["untested_files"], indent=0) + "\n")
    print(f"wrote {wiki / 'Code-Health.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
