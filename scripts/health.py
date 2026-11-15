#!/usr/bin/env python3
"""Measure the code health of a checkout and report what a change does to it.

The measures, their goals and the reasons for them are in
docs/architecture/health-goals.json; docs/architecture/HEALTH.md explains how
to read the results.

  python3 scripts/health.py measure [--cargo] [--coverage] [--out FILE]
      Measure this checkout and write JSON (to stdout, or FILE). The source
      measures take seconds. --cargo adds clippy, rustfmt, compiler warnings,
      the test count, dependencies and RustSec advisories (minutes);
      --coverage adds line coverage with cargo-llvm-cov (as long as the tests).

  python3 scripts/health.py diff BASE_DIR [--markdown]
      Compare this checkout with another one (for CI, the pull request's base
      commit) on the source measures: what got better, what got worse, and
      where. Never fails; in GitHub Actions it also writes warnings on the
      lines that made a measure worse.

  python3 scripts/health.py lint CLIPPY_JSON FMT_LIST CHANGED_FILES [--markdown]
      Report the clippy warnings (from `cargo clippy --message-format=json`)
      and the files rustfmt would change (from `cargo fmt -- --check -l`) that
      are in the files a pull request touches (one path per line).

  python3 scripts/health.py credits FROM TO [--repo DIR]
      Replay each first-parent commit in FROM..TO with the source measures
      and list, per pull request, the measures it improved. JSON to stdout.

Test code is skipped the same way as in scripts/arch_check.py, whose source
reader and layer rules this script uses.
"""
import functools
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import arch_check  # noqa: E402

# Everything read from one file is a pure function of its text, and one
# measurement reads every file several times (layering, module graph, items,
# sizes); a replay of consecutive commits reads mostly unchanged files. Caching
# by text makes both fast. The cache is bounded (about two trees of files), so
# a replay of hundreds of commits does not keep every old file in memory.
# arch_check.measure looks the names up in its module at call time, so it uses
# the cached versions too.
CACHE_FILES = 2048


def cached(fn):
    return functools.lru_cache(maxsize=CACHE_FILES)(fn)


arch_check.blank = cached(arch_check.blank)
_drop = cached(arch_check.drop_test_blocks)
arch_check.drop_test_blocks = lambda code: (lambda r: (r[0], list(r[1])))(_drop(code))
_references = cached(lambda code, _read=arch_check.references: tuple(_read(code)))
arch_check.references = lambda code: _references(code)

LONG_FUNCTION = 150       # lines, signature to closing brace
LARGE_FILE = 3000         # lines
PANIC = re.compile(r"\.unwrap\(\)|\.expect\(|\bpanic!\s*\(")
UNSAFE = re.compile(r"\bunsafe\s*(\{|fn\b|impl\b)")
FN = re.compile(r"\bfn\s+([A-Za-z_]\w*)")
GENERATED = re.compile(r"^\s*//[!/]?\s*(?:@generated\b|Generated\b)", re.M)
FILE_LOAD = ("io/",)      # file import and export: a panic here is a crash on a bad file

# Source measures: fast, per pull request, credited to the pull request.
SOURCE_MEASURES = [
    "upward_pairs", "upward_files", "mutual_pairs", "large_files", "long_functions",
    "message_variants", "panic_sites", "panic_sites_file_load", "unsafe_undocumented",
]


# ---- Reading a checkout ------------------------------------------------------

def production_files(root: Path):
    """(rel, top module, raw text, code) for each production .rs file;
    code has comments, strings and #[cfg(test)] blocks blanked, keeping lines."""
    src = root / "src"
    files = sorted(p.relative_to(src) for p in src.rglob("*.rs"))
    raw, codes, test_files = {}, {}, set()
    for rel in files:
        raw[rel] = (src / rel).read_text(encoding="utf-8", errors="replace")
        code, external = arch_check.drop_test_blocks(arch_check.blank(raw[rel]))
        codes[rel] = code
        for name in external:
            test_files.update(arch_check.child_paths(rel, name))
    # Same rule as arch_check.measure (files below a test module are test
    # code), compared as strings: Path.parents is slow over a thousand files.
    prefixes = tuple((t.parent if t.name == "mod.rs" else t.with_suffix("")).as_posix() + "/" for t in test_files)
    out = []
    for rel in files:
        if arch_check.named_as_test(rel) or rel in test_files or (prefixes and rel.as_posix().startswith(prefixes)):
            continue
        out.append((rel, arch_check.top_module(rel), raw[rel], codes[rel]))
    return out


def line_of(code: str, pos: int) -> int:
    return code.count("\n", 0, pos) + 1


def functions(code: str):
    """(name, first line, line count) for each fn with a body."""
    return _functions(code)


@cached
def _functions(code: str):
    return tuple(_scan_functions(code))


def _scan_functions(code: str):
    for m in FN.finditer(code):
        j = m.end()
        depth = 0
        while j < len(code):
            c = code[j]
            if code.startswith("->", j):
                j += 2          # the arrow's `>` closes nothing
                continue
            if c in "(<[":
                depth += 1
            elif c in ")>]":
                depth -= 1
            elif depth <= 0 and c == ";":
                break           # a declaration without a body
            elif depth <= 0 and c == "{":
                body = arch_check.braced(code, j)
                start = line_of(code, m.start())
                yield m.group(1), start, code.count("\n", m.start(), j + len(body) + 2) + 1
                break
            j += 1


def generated(raw: str) -> bool:
    """A file whose opening comment says it is generated (`//! Generated ...`
    or the `@generated` marker), within its first five lines. Its size is a
    property of the generator, so the size measures skip it."""
    return bool(GENERATED.search("\n".join(raw.split("\n", 5)[:5])))


def unsafe_documented(raw_lines, line: int, block: bool) -> bool:
    """Whether the comments directly above an unsafe item (or its own line)
    say why it is sound: `SAFETY:` for a block (clippy's
    undocumented_unsafe_blocks), `SAFETY:` or a `# Safety` doc section for an
    unsafe fn or impl (clippy's missing_safety_doc). Attributes between the
    comment and the item are allowed."""
    if "SAFETY:" in raw_lines[line - 1]:
        return True
    pattern = r"SAFETY:" if block else r"SAFETY:|#\s*Safety"
    j = line - 2
    while j >= 0 and re.match(r"\s*(//|#\[|\*|/\*)", raw_lines[j]):
        if re.search(pattern, raw_lines[j]):
            return True
        j -= 1
    return False


def message_variants(root: Path):
    path = root / "src" / "app" / "mod.rs"
    if not path.exists():
        return None
    code = arch_check.blank(path.read_text(encoding="utf-8", errors="replace"))
    m = re.search(r"^pub enum Message\b[^{]*\{", code, re.M)
    if not m:
        return None
    return len(re.findall(r"^    [A-Z][A-Za-z0-9]*\b", arch_check.braced(code, m.end() - 1), re.M))


def module_graph(root: Path, files=None):
    """All references between top-level modules in production code:
    {(src, dst): files}."""
    graph = defaultdict(set)
    for rel, mod, _, code in files if files is not None else production_files(root):
        if mod is None:
            continue
        for dst, _, _ in arch_check.references(code):
            if dst != mod and dst in arch_check.LAYERS:
                graph[(mod, dst)].add(f"src/{rel.as_posix()}")
    return graph


@cached
def line_findings(code: str, raw: str):
    """One file's panic sites and undocumented unsafe, each as (line, text)."""
    raw_lines = raw.split("\n")
    panics, unsafe_undoc = [], []
    for m in PANIC.finditer(code):
        line = line_of(code, m.start())
        panics.append((line, raw_lines[line - 1].strip()[:160]))
    for m in UNSAFE.finditer(code):
        line = line_of(code, m.start())
        if not unsafe_documented(raw_lines, line, block=m.group(1) == "{"):
            unsafe_undoc.append((line, raw_lines[line - 1].strip()[:160]))
    return tuple(panics), tuple(unsafe_undoc)


def measure_source(root: Path):
    """The source measures, with the findings behind each."""
    edges, unlayered = arch_check.measure(root)
    prod = production_files(root)
    graph = module_graph(root, prod)
    mutual = sorted({tuple(sorted(p)) for p in graph if (p[1], p[0]) in graph})

    large, long_fns, panics, unsafe_undoc, skipped = {}, [], [], [], []
    for p in sorted((root / "src").rglob("*.rs")):
        text = p.read_text(encoding="utf-8", errors="replace")
        key = f"src/{p.relative_to(root / 'src').as_posix()}"
        if generated(text):
            skipped.append(key)
            continue
        n = len(text.splitlines())
        if n > LARGE_FILE:
            large[key] = n
    for rel, mod, raw, code in prod:
        key = f"src/{rel.as_posix()}"
        if key not in skipped:
            for name, start, n in functions(code):
                if n > LONG_FUNCTION:
                    long_fns.append({"file": key, "line": start, "name": name, "lines": n})
        file_panics, file_unsafe = line_findings(code, raw)
        panics += [{"file": key, "line": line, "text": text} for line, text in file_panics]
        unsafe_undoc += [{"file": key, "line": line, "text": text} for line, text in file_unsafe]

    file_load = [x for x in panics if x["file"].startswith(tuple("src/" + p for p in FILE_LOAD))]
    values = {
        "upward_pairs": len(edges),
        "upward_files": sum(len(f) for f in edges.values()),
        "mutual_pairs": len(mutual),
        "large_files": len(large),
        "long_functions": len(long_fns),
        "message_variants": message_variants(root),
        "panic_sites": len(panics),
        "panic_sites_file_load": len(file_load),
        "unsafe_undocumented": len(unsafe_undoc),
    }
    details = {
        "upward": {pair: dict(sorted(fs.items())) for pair, fs in sorted(edges.items())},
        "upward_items": upward_items(root, prod),
        "unlayered": unlayered,
        "generated_files": skipped,
        "mutual_pairs": [list(p) for p in mutual],
        "large_files": dict(sorted(large.items(), key=lambda kv: -kv[1])),
        "long_functions": sorted(long_fns, key=lambda f: -f["lines"]),
        "panic_sites": panics,
        "unsafe_undocumented": unsafe_undoc,
    }
    return values, details


def item_paths(rest: str, depth=0):
    """Every path named after a module reference: `::overlay::X` gives
    ["overlay::X"]; `::{A, b::C}` gives ["A", "b::C"]; a bare module or
    `self` gives [""]."""
    rest = rest.lstrip()
    if not rest.startswith("::") or depth > 6:
        return [""]
    rest = rest[2:].lstrip()
    if rest.startswith("{"):
        out = []
        for part in arch_check.split_top(arch_check.braced(rest, 0)):
            m = re.match(r"([A-Za-z_]\w*)(.*)", part, re.S)
            if m and m.group(1) != "self":
                out += [m.group(1) + ("::" + p if p else "") for p in item_paths(m.group(2), depth + 1)]
            elif m:
                out.append("")
        return out or [""]
    m = re.match(r"[A-Za-z_]\w*", rest)
    if not m:
        return [""]
    return [m.group(0) + ("::" + p if p else "") for p in item_paths(rest[m.end():], depth + 1)]


def references_with_paths(code: str):
    """arch_check.references plus the full item paths of each reference."""
    return _references_with_paths(code)


@cached
def _references_with_paths(code: str):
    return tuple(_scan_references_with_paths(code))


def _scan_references_with_paths(code: str):
    for m in arch_check.REF.finditer(code):
        tail = code[m.end():m.end() + 2000]
        yield m.group(1), arch_check.first_items(tail), item_paths(tail), line_of(code, m.start())
    for m in arch_check.USE_GROUP.finditer(code):
        line = line_of(code, m.start())
        for part in arch_check.split_top(arch_check.braced(code, m.end() - 1)):
            head = re.match(r"([a-z_][a-z0-9_]*)\b(.*)", part, re.S)
            if head:
                yield head.group(1), arch_check.first_items(head.group(2)), item_paths(head.group(2)), line


def upward_items(root: Path, files=None):
    """For each upward pair: {file: [first items named]}; the input for the
    'pick up a cleanup' list (a pair used by one file for one item is a small
    move)."""
    out = defaultdict(dict)
    for rel, mod, _, code in files if files is not None else production_files(root):
        if mod is None:
            continue
        for dst, items, paths, line in references_with_paths(code):
            if dst == mod or not arch_check.upward(mod, dst) or arch_check.allowed(mod, dst, items):
                continue
            entry = out[f"{mod} -> {dst}"].setdefault(f"src/{rel.as_posix()}", {"line": line, "items": []})
            entry["line"] = min(entry["line"], line)
            for i in paths:
                i = i or "(module)"
                # `Type::method` and `Enum::Variant` name the item `Type`.
                parts = i.split("::")
                cut = next((k + 1 for k, seg in enumerate(parts) if seg[:1].isupper() or seg.isupper()), len(parts))
                i = "::".join(parts[:cut])
                if i not in entry["items"]:
                    entry["items"].append(i)
    return {k: dict(sorted(v.items())) for k, v in sorted(out.items())}


# ---- Cargo measures ------------------------------------------------------------

def run(cmd, cwd, **kw):
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, **kw)


def parse_clippy(lines):
    """Clippy and compiler warnings from `--message-format=json` output,
    deduplicated (a file built for several targets reports twice)."""
    seen, clippy, rustc = set(), [], []
    for line in lines:
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") != "compiler-message":
            continue
        m = msg["message"]
        if m.get("level") != "warning" or not m.get("spans"):
            continue
        span = next((s for s in m["spans"] if s.get("is_primary")), m["spans"][0])
        code = (m.get("code") or {}).get("code") or ""
        fixable = any(s.get("suggestion_applicability") == "MachineApplicable"
                      for c in [m] + m.get("children", []) for s in c.get("spans", []))
        key = (span["file_name"], span["line_start"], code, m["message"])
        if key in seen:
            continue
        seen.add(key)
        item = {"file": span["file_name"], "line": span["line_start"], "code": code,
                "message": m["message"], "fixable": fixable}
        (clippy if code.startswith("clippy::") else rustc).append(item)
    return clippy, rustc


def measure_cargo(root: Path, coverage: bool):
    values, details = {}, {}
    out = run(["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--message-format=json"], root)
    clippy, rustc = parse_clippy(out.stdout.splitlines())
    values["clippy_warnings"], values["compiler_warnings"] = len(clippy), len(rustc)
    details["clippy_by_lint"] = Counter(w["code"] for w in clippy).most_common()
    details["clippy_fixable_by_lint"] = Counter(w["code"] for w in clippy if w["fixable"]).most_common()
    details["clippy_fixable"] = sum(w["fixable"] for w in clippy)

    fmt = run(["cargo", "fmt", "--all", "--", "--check", "-l"], root)
    files = sorted({l.strip() for l in fmt.stdout.splitlines() if l.strip().endswith(".rs")})
    values["rustfmt_files"] = len(files)
    details["rustfmt_files"] = [os.path.relpath(f, root) if os.path.isabs(f) else f for f in files]

    tests = run(["cargo", "test", "--workspace", "--locked", "--", "--list", "--format", "terse"], root)
    values["tests"] = sum(1 for l in tests.stdout.splitlines() if l.endswith(": test"))

    meta = run(["cargo", "metadata", "--format-version", "1", "--locked"], root)
    if meta.returncode == 0:
        m = json.loads(meta.stdout)
        members = set(m["workspace_members"])
        direct = {d["name"] for p in m["packages"] if p["id"] in members for d in p["dependencies"]}
        local = {p["name"] for p in m["packages"] if p["id"] in members}
        values["dependencies_direct"] = len(direct - local)
        values["dependencies_total"] = len(m["packages"]) - len(members)

    audit = run(["cargo", "audit", "--json"], root)
    try:
        a = json.loads(audit.stdout)
        values["advisories"] = a["vulnerabilities"]["count"] + sum(len(v) for v in a.get("warnings", {}).values())
        details["advisories"] = [v["advisory"]["id"] for v in a["vulnerabilities"]["list"]]
    except (ValueError, KeyError):
        details["advisories_error"] = "cargo-audit not available"

    if coverage:
        cov = run(["cargo", "llvm-cov", "--workspace", "--locked", "--summary-only", "--json"], root)
        try:
            values["coverage"] = round(json.loads(cov.stdout)["data"][0]["totals"]["lines"]["percent"], 1)
        except (ValueError, KeyError, IndexError):
            details["coverage_error"] = "cargo-llvm-cov not available"
    return values, details


def measure(root: Path, cargo=False, coverage=False):
    values, details = measure_source(root)
    if cargo:
        v, d = measure_cargo(root, coverage)
        values.update(v)
        details.update(d)
    commit = run(["git", "rev-parse", "HEAD"], root).stdout.strip() or None
    return {"commit": commit, "values": values, "details": details}


# ---- Comparing two checkouts -----------------------------------------------------

LABELS = {
    "upward_pairs": "upward module pairs", "upward_files": "upward dependencies (files)",
    "mutual_pairs": "modules that use each other", "large_files": "files over 3,000 lines",
    "long_functions": "functions over 150 lines", "message_variants": "top-level Message variants",
    "panic_sites": "unwrap/expect/panic! in production code",
    "panic_sites_file_load": "unwrap/expect/panic! in file import and export (src/io)",
    "unsafe_undocumented": "unsafe without a SAFETY comment",
}


def new_lines(base, head, key=lambda x: (x["file"], x["text"])):
    """Findings in head whose (file, text) occurs more often than in base:
    robust to lines moving."""
    left = Counter(key(x) for x in base)
    out = []
    for x in head:
        k = key(x)
        if left[k]:
            left[k] -= 1
        else:
            out.append(x)
    return out


def compare(base, head):
    """What changed: measure deltas plus the findings behind the worse ones."""
    deltas = {k: (base["values"][k], head["values"][k]) for k in SOURCE_MEASURES
              if None not in (base["values"][k], head["values"][k]) and base["values"][k] != head["values"][k]}
    bd, hd = base["details"], head["details"]
    added = {
        "upward": sorted((p, f, l) for p, fs in hd["upward"].items() for f, l in fs.items()
                         if f not in bd["upward"].get(p, {})),
        "panic_sites": new_lines(bd["panic_sites"], hd["panic_sites"]),
        "unsafe_undocumented": new_lines(bd["unsafe_undocumented"], hd["unsafe_undocumented"]),
        # Only functions that crossed the limit: one that was already long and
        # grew is the existing debt, not this change's.
        "long_functions": new_lines(bd["long_functions"], hd["long_functions"],
                                    key=lambda f: (f["file"], f["name"])),
        "large_files": [f for f in hd["large_files"] if f not in bd["large_files"]],
        "mutual_pairs": [p for p in hd["mutual_pairs"] if p not in bd["mutual_pairs"]],
    }
    removed_upward = sorted((p, f) for p, fs in bd["upward"].items() for f in fs if f not in hd["upward"].get(p, {}))
    return deltas, added, removed_upward


def better(k, a, b):
    return b < a


def diff(base_dir: str, markdown: bool) -> int:
    base, head = measure(Path(base_dir)), measure(Path.cwd())
    deltas, added, removed_upward = compare(base, head)
    good = [(k, a, b) for k, (a, b) in deltas.items() if better(k, a, b)]
    bad = [(k, a, b) for k, (a, b) in deltas.items() if not better(k, a, b)]
    if markdown:
        print("### Code health\n")
        if not deltas:
            print("No change to the source measures.\n")
        if good:
            print("**Thank you: this pull request improves**\n")
            for k, a, b in good:
                print(f"- {LABELS[k]}: {a:,} → {b:,}")
            print()
        if bad:
            print("**Made worse** (each is listed below with where it is)\n")
            for k, a, b in bad:
                print(f"- {LABELS[k]}: {a:,} → {b:,}")
            print()
        sections = [
            ("Upward dependencies removed", [f"`{f}` ({p})" for p, f in removed_upward]),
            ("New upward dependencies", [f"`{f}:{l}` ({p})" for p, f, l in added["upward"]]),
            ("New unwrap/expect/panic!", [f"`{x['file']}:{x['line']}`" for x in added["panic_sites"]]),
            ("New unsafe without a SAFETY comment", [f"`{x['file']}:{x['line']}`" for x in added["unsafe_undocumented"]]),
            ("Functions now over 150 lines", [f"`{x['file']}:{x['line']}` `{x['name']}` ({x['lines']} lines)" for x in added["long_functions"]]),
            ("Files now over 3,000 lines", [f"`{f}` ({head['details']['large_files'][f]:,} lines)" for f in added["large_files"]]),
            ("Modules that now use each other", [f"`{a}` and `{b}`" for a, b in added["mutual_pairs"]]),
        ]
        for title, items in sections:
            if items:
                print(f"<details><summary>{title} ({len(items)})</summary>\n")
                for i in items:
                    print(f"- {i}")
                print("\n</details>\n")
        print("This report never blocks merging. Goals and reasons: "
              "`docs/architecture/health-goals.json`; how to read it: `docs/architecture/HEALTH.md`.")
    else:
        for k, (a, b) in deltas.items():
            print(f"  {'better' if better(k, a, b) else 'worse ':6s} {LABELS[k]:45s} {a:>6} -> {b}")
        if os.environ.get("GITHUB_ACTIONS") == "true":
            notes = ([(f, l, "Upward dependency", f"{p}; see docs/architecture/RULES.md") for p, f, l in added["upward"]]
                     + [(x["file"], x["line"], "New panic site", "unwrap/expect/panic! in production code; prefer returning an error")
                        for x in added["panic_sites"]]
                     + [(x["file"], x["line"], "Undocumented unsafe", "add a // SAFETY: comment explaining why this is sound")
                        for x in added["unsafe_undocumented"]]
                     + [(x["file"], x["line"], "Long function", f"{x['name']} is {x['lines']} lines (guide: {LONG_FUNCTION})")
                        for x in added["long_functions"]])
            for f, l, title, msg in notes:
                print(f"::warning file={f},line={l},title={title}::{msg}")
    return 0


def lint(clippy_json: str, fmt_list: str, changed: str, markdown: bool) -> int:
    touched = {l.strip() for l in Path(changed).read_text().splitlines() if l.strip()}
    clippy, rustc = parse_clippy(Path(clippy_json).read_text().splitlines())
    mine = [w for w in clippy + rustc if w["file"] in touched]
    fmt = sorted({l.strip() for l in Path(fmt_list).read_text().splitlines() if l.strip()})
    fmt_mine = [f for f in fmt if any(f.endswith("/" + t) or f == t for t in touched)]
    if markdown:
        print("### Warnings in the files this pull request touches\n")
        if not mine and not fmt_mine:
            print("None. Thank you for leaving these files clean.\n")
        if mine:
            fix = sum(w["fixable"] for w in mine)
            print(f"{len(mine)} warning(s) in {len({w['file'] for w in mine})} file(s); {fix} can be fixed "
                  f"automatically with `cargo clippy --fix`. Fixing a few while you are in the file helps.\n")
            print("| Lint | Count |\n| --- | ---: |")
            for code, n in Counter(w["code"] or "rustc" for w in mine).most_common():
                print(f"| `{code}` | {n} |")
            print()
        if fmt_mine:
            print(f"rustfmt would change {len(fmt_mine)} touched file(s): run `cargo fmt` on them.\n")
    else:
        for w in mine:
            print(f"  {w['file']}:{w['line']} {w['code'] or 'rustc'} {w['message']}")
    return 0


# ---- Credits -----------------------------------------------------------------

def pr_author(number: int):
    """The pull request author's login from the GitHub API, when the `gh`
    tool and a token are available (as in CI); otherwise None."""
    repo = os.environ.get("GITHUB_REPOSITORY")
    if not repo or not os.environ.get("GH_TOKEN"):
        return None
    try:
        r = run(["gh", "api", f"repos/{repo}/pulls/{number}", "--jq", ".user.login"], ".", timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return r.stdout.strip() or None if r.returncode == 0 else None


MERGE_SUBJECT = re.compile(r"^Merge (?:pull request #(\d+)\b|#(\d+)\b|branch '?pr-?(\d+)\b)", re.I)


def merged_pr_number(subject: str):
    """The pull request a merge commit merges, from GitHub's merge subjects
    ("Merge pull request #12 from ...", "Merge #12: ...", "Merge branch
    'pr12'"). Any other number in a subject may be an issue, so it is not
    taken."""
    m = MERGE_SUBJECT.search(subject)
    return int(next(g for g in m.groups() if g)) if m else None


def pr_for_commit(sha: str):
    """The merged pull request a direct commit (for example a squash merge)
    came from, from the GitHub API when the `gh` tool and a token are
    available (as in CI); otherwise None, and no pull request is linked."""
    repo = os.environ.get("GITHUB_REPOSITORY")
    if not repo or not os.environ.get("GH_TOKEN"):
        return None
    try:
        r = run(["gh", "api", f"repos/{repo}/commits/{sha}/pulls",
                 "--jq", "[.[] | select(.merged_at != null) | .number][0] // empty"], ".", timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return int(r.stdout.strip()) if r.returncode == 0 and r.stdout.strip().isdigit() else None


def credits(rev_from: str, rev_to: str, repo: Path):
    """Per first-parent commit in FROM..TO, the source measures it improved."""
    revs = run(["git", "rev-list", "--first-parent", "--reverse", f"{rev_from}..{rev_to}"], repo).stdout.split()
    out, prev = [], None
    with tempfile.TemporaryDirectory() as tmp:
        def at(rev):
            dest = Path(tmp) / rev
            dest.mkdir()
            archive = subprocess.run(["git", "-C", str(repo), "archive", "--format=tar", rev, "src"],
                                     check=True, capture_output=True).stdout
            with tarfile.open(fileobj=io.BytesIO(archive)) as t:
                t.extractall(dest, filter="data")
            try:
                return measure_source(dest)[0]
            finally:
                shutil.rmtree(dest)       # hundreds of trees would fill the disk
        prev = at(run(["git", "rev-parse", rev_from], repo).stdout.strip())
        for rev in revs:
            touched = run(["git", "diff", "--name-only", f"{rev}^1", rev, "--", "src"], repo).stdout.split()
            if not touched:
                continue
            cur = at(rev)
            improved = {k: [prev[k], cur[k]] for k in SOURCE_MEASURES
                        if prev[k] is not None and cur[k] is not None and cur[k] < prev[k]}
            if improved:
                subj = run(["git", "log", "-1", "--format=%s%x00%an", rev], repo).stdout.strip().split("\0")
                # A merge's own author is whoever merged; the pull request's
                # author is on its second parent.
                pr_side = run(["git", "log", "-1", "--format=%an", f"{rev}^2"], repo)
                merge = pr_side.returncode == 0 and bool(pr_side.stdout.strip())
                if merge:
                    subj[1] = pr_side.stdout.strip()
                number = merged_pr_number(subj[0]) if merge else pr_for_commit(rev)
                login = re.search(r"^Merge pull request #\d+ from ([A-Za-z0-9-]+)/", subj[0]) if merge else None
                login = login.group(1) if login else (pr_author(number) if number else None)
                out.append({"commit": rev, "subject": subj[0], "pr": number,
                            "login": login, "author": subj[1], "improved": improved})
            prev = cur
    return out


# ---- Command line --------------------------------------------------------------

def main(argv) -> int:
    flags, args, options, i = set(), [], {}, 0
    while i < len(argv):
        a = argv[i]
        if a in ("--out", "--repo") and i + 1 < len(argv):
            options[a] = argv[i + 1]
            i += 2
            continue
        (flags.add if a.startswith("--") else args.append)(a)
        i += 1
    if args[:1] == ["measure"]:
        out_file = options.get("--out")
        data = measure(Path.cwd(), cargo="--cargo" in flags, coverage="--coverage" in flags)
        text = json.dumps(data, indent=1) + "\n"
        if out_file:
            Path(out_file).write_text(text)
        else:
            sys.stdout.write(text)
        return 0
    if args[:1] == ["diff"] and len(args) == 2:
        return diff(args[1], "--markdown" in flags)
    if args[:1] == ["lint"] and len(args) == 4:
        return lint(args[1], args[2], args[3], "--markdown" in flags)
    if args[:1] == ["credits"] and len(args) == 3:
        repo = Path(options.get("--repo", "."))
        json.dump(credits(args[1], args[2], repo), sys.stdout, indent=1)
        print()
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
