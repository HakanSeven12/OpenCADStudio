#!/usr/bin/env python3
"""Report dependencies that point up the module layers.

Every top-level module under src/ is assigned a layer (LAYERS below; the
reasons are in docs/architecture/RULES.md). Code may use modules in its own
layer or lower ones. A reference from a module to one in a higher layer is an
upward dependency.

  python3 scripts/arch_check.py report [--markdown]
      Count the upward dependencies in this checkout.

  python3 scripts/arch_check.py diff BASE_DIR [--markdown]
      Compare this checkout with another one (for CI, the pull request's base
      commit) and list the upward dependencies added and removed. Exits 1 if
      any were added; CI ignores the exit code and never blocks.

Counts are files: a file that refers to a higher module five times counts
once for that pair. Test code is skipped: files named tests.rs,
test_support.rs or *_tests.rs, files under a tests/ or *_tests/ folder,
#[cfg(test)] mod blocks, and files declared with `#[cfg(test)] mod name;`.
Comments and string contents are ignored.
"""
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

# ---- Rules as data -------------------------------------------------------

# Layer per top-level module. A module may use its own layer or lower.
LAYERS = {
    # 0 foundations: no knowledge of CAD or the app
    "par": 0, "perf": 0, "sys": 0, "sysinfo": 0, "i18n": 0, "config": 0,
    "network": 0, "schema_validation": 0,
    # 1 drawing model and geometry behaviour
    "entities": 1, "snap": 1, "parametric": 1, "entity_creation": 1,
    "filter": 1,
    # 2 file formats and rendering
    "io": 2, "scene": 2, "gpu_backend": 2,
    # 3 the tool contract and plugin runtime
    "command": 3, "plugin": 3,
    # 4 features: tools and ribbon tabs
    "modules": 4,
    # 5 presentation
    "ui": 5, "input": 5,
    # 6 the app shell and its front doors
    "app": 6, "mcp": 6, "mcp_intent": 6, "mcp_ops": 6, "rest": 6, "cli": 6,
    "discussions": 6, "patreon": 6, "videos": 6, "locale_catalog": 6,
}

# Same-layer pairs that are still upward: rendering must not depend on file
# formats, or the two become a cycle.
SAME_LAYER_UPWARD = {("scene", "io")}

# Items a lower module may use from a higher one by design. The views in `ui`
# return the application's message type; that is how iced works.
ALLOWED = {("ui", "app"): {"Message"}}

# ---- Reading Rust source -------------------------------------------------


def blank(text: str) -> str:
    """Remove comments and string contents, keeping every newline.

    Line numbers stay the same, so findings can point at the right line, and
    braces inside strings or comments cannot unbalance the test-block scan.
    """
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    if text[i] == "\n":
                        out.append("\n")
                    i += 1
        elif c == '"' or (c in "rb" and re.match(r'(?:br|r|b)#*"', text[i:i + 8])
                          and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_"))):
            m = re.match(r'(b?r)(#*)"', text[i:i + 8])
            if m:  # raw string: ends at the quote followed by the same hashes
                end = '"' + m.group(2)
                j = text.find(end, i + len(m.group(0)))
                j = n if j < 0 else j + len(end)
            else:  # ordinary or byte string, with escapes
                j = i + (2 if c == "b" else 1)
                while j < n and text[j] != '"':
                    j += 2 if text[j] == "\\" else 1
                j += 1
            out.append('""' + "\n" * text.count("\n", i, j))
            i = j
        elif c == "'":
            m = re.match(r"'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^\\'])'", text[i:i + 12])
            if m:  # a char literal; otherwise a lifetime or label
                out.append("' '")
                i += len(m.group(0))
            else:
                out.append(c)
                i += 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


TEST_ATTR = re.compile(r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*([{;])")


def drop_test_blocks(code: str):
    """Blank inline #[cfg(test)] mod blocks; return the code and the names of
    out-of-line test modules (`#[cfg(test)] mod name;`)."""
    out, i, external = [], 0, []
    for m in TEST_ATTR.finditer(code):
        if m.start() < i:
            continue
        if m.group(2) == ";":
            external.append(m.group(1))
            continue
        out.append(code[i:m.start()])
        depth, j = 1, m.end()
        while j < len(code) and depth:
            depth += (code[j] == "{") - (code[j] == "}")
            j += 1
        out.append("\n" * code.count("\n", m.start(), j))
        i = j
    out.append(code[i:])
    return "".join(out), external


def split_top(group: str):
    """Split the inside of a `{...}` use group at its top-level commas."""
    parts, depth, cur = [], 0, []
    for ch in group:
        if ch == "," and depth == 0:
            parts.append("".join(cur).strip())
            cur = []
            continue
        depth += (ch == "{") - (ch == "}")
        cur.append(ch)
    parts.append("".join(cur).strip())
    return [p for p in parts if p]


def braced(code: str, start: int) -> str:
    """The text inside the braces that open at code[start]."""
    depth, j = 1, start + 1
    while j < len(code) and depth:
        depth += (code[j] == "{") - (code[j] == "}")
        j += 1
    return code[start + 1:j - 1]


def first_items(rest: str):
    """The first path segment(s) after a module name: `Message` for
    `::Message::Open`, both names for `::{Message, settings}`, and None for a
    bare module reference."""
    rest = rest.strip()
    if not rest.startswith("::"):
        return None
    rest = rest[2:].lstrip()
    if rest.startswith("{"):
        return [re.split(r"\W", p, maxsplit=1)[0] for p in split_top(braced(rest, 0))]
    m = re.match(r"[A-Za-z_]\w*", rest)
    return [m.group(0)] if m else None


REF = re.compile(r"\bcrate\s*::\s*([a-z_][a-z0-9_]*)\b")
USE_GROUP = re.compile(r"\buse\s+crate\s*::\s*\{")


def references(code: str):
    """Yield (module, items, line) for each reference to a crate module."""
    for m in REF.finditer(code):
        line = code.count("\n", 0, m.start()) + 1
        yield m.group(1), first_items(code[m.end():m.end() + 2000]), line
    for m in USE_GROUP.finditer(code):
        start = m.end() - 1
        line = code.count("\n", 0, m.start()) + 1
        for part in split_top(braced(code, start)):
            head = re.match(r"([a-z_][a-z0-9_]*)\b(.*)", part, re.S)
            if head:
                yield head.group(1), first_items(head.group(2)), line


def upward(src: str, dst: str) -> bool:
    if (src, dst) in SAME_LAYER_UPWARD:
        return True
    a, b = LAYERS.get(src), LAYERS.get(dst)
    return a is not None and b is not None and b > a


def allowed(src: str, dst: str, items) -> bool:
    ok = ALLOWED.get((src, dst))
    return bool(ok and items and all(i in ok for i in items))


# ---- Measuring a checkout -------------------------------------------------


def top_module(rel: Path):
    """The crate module a file belongs to; None for the crate roots and for
    separate binaries under src/bin/."""
    if len(rel.parts) == 1:
        return None if rel.stem in ("main", "lib") else rel.stem
    return None if rel.parts[0] == "bin" else rel.parts[0]


def named_as_test(rel: Path) -> bool:
    n = rel.name
    return (n.endswith("_tests.rs") or n in ("tests.rs", "test_support.rs")
            or any(x == "tests" or x.endswith("_tests") for x in rel.parts[:-1]))


def child_paths(rel: Path, name: str):
    """Where `mod name;` declared in `rel` lives."""
    base = rel.parent if rel.name in ("mod.rs", "lib.rs", "main.rs") else rel.with_suffix("")
    return [base / f"{name}.rs", base / name / "mod.rs"]


def measure(root: Path):
    """Return {pair: {file: first_line}} and the modules with no layer."""
    src = root / "src"
    files = sorted(p.relative_to(src) for p in src.rglob("*.rs"))
    codes, test_files = {}, set()
    for rel in files:
        code, external = drop_test_blocks(blank((src / rel).read_text(encoding="utf-8", errors="replace")))
        codes[rel] = code
        for name in external:
            test_files.update(child_paths(rel, name))
    # Files below a test module (its own `mod x;` children) are test code too.
    test_dirs = {t.parent if t.name == "mod.rs" else t.with_suffix("") for t in test_files}
    test_files.update(rel for rel in files if any(d in rel.parents for d in test_dirs))

    edges = defaultdict(dict)
    unlayered = set()
    for rel in files:
        mod = top_module(rel)
        if mod is None:
            continue
        if mod not in LAYERS:
            unlayered.add(mod)
        if named_as_test(rel) or rel in test_files:
            continue
        for dst, items, line in references(codes[rel]):
            if dst == mod or not upward(mod, dst) or allowed(mod, dst, items):
                continue
            pair = f"{mod} -> {dst}"
            key = f"src/{rel.as_posix()}"
            edges[pair][key] = min(line, edges[pair].get(key, line))
    return dict(edges), sorted(unlayered)


# ---- Output ----------------------------------------------------------------


def layer_note(pair: str) -> str:
    a, b = pair.split(" -> ")
    return f"`{a}` (layer {LAYERS[a]}) uses `{b}` (layer {LAYERS[b]})"


def totals(edges):
    return len(edges), sum(len(f) for f in edges.values())


def counted(pairs: int, files: int) -> str:
    return (f"{pairs} module pair{'' if pairs == 1 else 's'}, "
            f"{files} file{'' if files == 1 else 's'}")


def report(markdown: bool) -> int:
    edges, unlayered = measure(Path.cwd())
    pairs, files = totals(edges)
    rows = sorted(edges.items(), key=lambda kv: (-len(kv[1]), kv[0]))
    if markdown:
        print(f"### Upward dependencies: {counted(pairs, files)}\n")
        print("| Pair | Files |\n| --- | ---: |")
        for pair, fs in rows:
            print(f"| `{pair}` | {len(fs)} |")
    else:
        print(f"Upward dependencies: {counted(pairs, files)}")
        for pair, fs in rows:
            print(f"  {pair:34s}{len(fs):>5}")
    if unlayered:
        print(f"\nModules with no layer (add them to LAYERS): {', '.join(unlayered)}")
    return 0


def diff(base_dir: str, markdown: bool) -> int:
    base, _ = measure(Path(base_dir))
    head, unlayered = measure(Path.cwd())
    added = sorted((pair, f, line) for pair, fs in head.items()
                   for f, line in fs.items() if f not in base.get(pair, {}))
    removed = sorted((pair, f) for pair, fs in base.items()
                     for f in fs if f not in head.get(pair, {}))
    new_pairs = sorted(set(head) - set(base))
    gone_pairs = sorted(set(base) - set(head))
    (bp, bf), (hp, hf) = totals(base), totals(head)

    if markdown:
        print("### Upward dependencies\n")
        print(f"Base: {counted(bp, bf)}. This pull request: {counted(hp, hf)}.\n")
        if not added and not removed:
            print("No change.")
        if added:
            print("**Added**\n")
            for pair, f, line in added:
                new = " (new pair)" if pair in new_pairs else ""
                print(f"- `{pair}`{new}: `{f}:{line}`")
            print()
        if removed:
            print("**Removed**\n")
            for pair, f in removed:
                gone = " (pair now gone)" if pair in gone_pairs else ""
                print(f"- `{pair}`{gone}: `{f}`")
            print()
        print("This check only reports; it does not block merging. "
              "See `docs/architecture/RULES.md`.")
    else:
        print(f"Upward dependencies: base {counted(bp, bf)}; this tree {counted(hp, hf)}")
        for pair, f, line in added:
            print(f"  added    {pair:30s} {f}:{line}")
        for pair, f in removed:
            print(f"  removed  {pair:30s} {f}")
        if os.environ.get("GITHUB_ACTIONS") == "true":
            for pair, f, line in added:
                print(f"::warning file={f},line={line},title=Upward dependency::"
                      f"{layer_note(pair).replace('`', '')}. See docs/architecture/RULES.md.")
    if unlayered:
        print(f"\nModules with no layer (add them to LAYERS): {', '.join(unlayered)}")
    return 1 if added else 0


def main(argv) -> int:
    args = [a for a in argv if a != "--markdown"]
    markdown = "--markdown" in argv
    if args[:1] == ["report"] or not args:
        return report(markdown)
    if args[:1] == ["diff"] and len(args) == 2:
        return diff(args[1], markdown)
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
