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
      any were added or a module has no layer; CI fails the change then (the
      `architecture` job).

  python3 scripts/arch_check.py readiness [--markdown]
      For each crate planned in docs/architecture/TARGET.md (CRATES below),
      list what its modules use that is neither in the crate nor in a crate
      below it. Production code and test code are counted apart, because the
      tests move with their crate. Never fails.

Counts are files: a file that refers to a higher module five times counts
once for that pair. Test code is skipped: files named tests.rs,
test_support.rs or *_tests.rs, files under a tests/ or *_tests/ folder,
#[cfg(test)] mod blocks, and files declared with `#[cfg(test)] mod name;`.
Comments and string contents are ignored.

The upward check reads `crate::module` paths only. The readiness report also
reads what that check does not see: items in the crate root (src/lib.rs),
`super::` paths that climb out of a top-level module, and the macros that
src/ exports (`crate::t!` is a use of `i18n`).
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
    "network": 0, "schema_validation": 0, "xml_depth": 0, "wildcard": 0, "paths": 0, "icon_assets": 0,
    # 1 drawing model and geometry behaviour
    "entities": 1, "display": 1, "text": 1, "parametric": 1,
    "filter": 1, "calc": 1, "catalog": 1, "media": 1,
    # 2 file formats and rendering
    "io": 2, "scene": 2, "snap": 2, "gpu_backend": 2, "ifc_plan": 2,
    # 3 the tool contract
    "command": 3, "plugin_catalog": 3,
    # 4 features: tools and ribbon tabs
    "modules": 4, "entity_creation": 4,
    # 5 presentation
    "ui": 5, "input": 5,
    # 6 the app shell and its front doors
    "app": 6, "plugin": 6, "mcp": 6, "mcp_intent": 6, "mcp_ops": 6, "rest": 6, "cli": 6,
    "discussions": 6, "patreon": 6, "videos": 6,
}

# Same-layer pairs that are still upward: rendering must not depend on file
# formats, or the two become a cycle.
SAME_LAYER_UPWARD = {("scene", "io")}

# Items a lower module may use from a higher one by design. The views in `ui`
# return the application's message type; that is how iced works.
ALLOWED = {("ui", "app"): {"Message"}}

# The crates planned in docs/architecture/TARGET.md, lowest first: name,
# layer, modules. The modules TARGET.md names are in their crate; each other
# module of layers 0-5 is in the crate of its layer. Layer 2 has three crates:
# file formats in ocs_io, rendering in ocs_render, and `catalog`, which both
# use, in none yet. A crate may use the crates of a lower layer and the ones in
# MAY_USE. Layer 6 is the root package.
CRATES = [
    ("ocs_base", 0, ["par", "perf", "i18n", "config", "network", "wildcard", "xml_depth",
                     "schema_validation", "sys", "sysinfo", "paths", "icon_assets"]),
    ("ocs_model", 1, ["entities", "parametric", "filter", "calc", "display", "text", "catalog",
                      "media"]),
    ("ocs_io", 2, ["io", "ifc_plan"]),
    ("ocs_session", 2, []),  # made by splitting `Scene` (TARGET.md, step 6)
    ("ocs_render", 2, ["scene", "gpu_backend", "snap"]),
    ("ocs_commands", 3, ["command", "plugin_catalog", "modules", "entity_creation"]),
    ("ocs_ui", 5, ["ui", "input"]),
]

# Same-layer crates a crate may use: rendering reads a session.
MAY_USE = {("ocs_render", "ocs_session")}

# The target of a use of an item defined in src/lib.rs.
CRATE_ROOT = "crate root"

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


TEST_ATTR = re.compile(r"#\[cfg\(((?:[^()\]]|\((?:[^()]|\([^()]*\))*\))*)\)\]\s*(?:#\[[^\]]*\]\s*)*"
                       r"(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*([{;])")


def test_only(cfg: str) -> bool:
    """Whether a cfg predicate holds only in test builds: `test`, or `all(...)`
    with `test` among its arguments, e.g. `all(test, not(target_arch = "wasm32"))`."""
    cfg = cfg.strip()
    if cfg == "test":
        return True
    m = re.fullmatch(r"all\((.*)\)", cfg, re.S)
    if not m:
        return False
    args, depth, cur = [], 0, ""
    for ch in m.group(1):
        if ch == "," and depth == 0:
            args.append(cur)
            cur = ""
            continue
        depth += (ch == "(") - (ch == ")")
        cur += ch
    return any(a.strip() == "test" for a in args + [cur])


def drop_test_blocks(code: str):
    """Blank inline #[cfg(test)] mod blocks; return the code and the names of
    out-of-line test modules (`#[cfg(test)] mod name;`)."""
    out, i, external = [], 0, []
    for m in TEST_ATTR.finditer(code):
        if not test_only(m.group(1)):
            continue
        if m.start() < i:
            continue
        if m.group(3) == ";":
            external.append(m.group(2))
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


def read_tree(root: Path):
    """Read every .rs file under src/. Return the sorted paths, the blanked
    text of each, the same text with its test blocks blanked too, and the
    files that are test code because a `#[cfg(test)] mod name;` declares them
    or a module below one."""
    src = root / "src"
    files = sorted(p.relative_to(src) for p in src.rglob("*.rs"))
    full, codes, test_files = {}, {}, set()
    for rel in files:
        full[rel] = blank((src / rel).read_text(encoding="utf-8", errors="replace"))
        code, external = drop_test_blocks(full[rel])
        codes[rel] = code
        for name in external:
            test_files.update(child_paths(rel, name))
    # Files below a test module (its own `mod x;` children) are test code too.
    # Compared as strings: Path.parents is slow over a thousand files.
    prefixes = tuple((t.parent if t.name == "mod.rs" else t.with_suffix("")).as_posix() + "/" for t in test_files)
    if prefixes:
        test_files.update(rel for rel in files if rel.as_posix().startswith(prefixes))
    return files, full, codes, test_files


def measure(root: Path):
    """Return {pair: {file: first_line}} and the modules with no layer."""
    files, _, codes, test_files = read_tree(root)
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


# ---- Crate readiness -------------------------------------------------------

ROOT_MOD = re.compile(r"\bmod\s+([A-Za-z_]\w*)")
ROOT_USE = re.compile(r"\buse\s+([^;]*);")
EXPORTED_MACRO = re.compile(r"#\[macro_export\]\s*(?:#\[[^\]]*\]\s*)*macro_rules!\s*([A-Za-z_]\w*)\s*\{")
# `$crate::x` in a macro means the crate that defines the macro; a plain
# `crate::x` means the crate of each caller.
CALLER_PATH = re.compile(r"(?<!\$)\bcrate\s*::\s*([A-Za-z_]\w*)")
CRATE_PATH = re.compile(r"\bcrate\s*::\s*(\{|[A-Za-z_]\w*)")
SUPER_PATH = re.compile(r"(?<![\w$])super(?:\s*::\s*super)*\s*::\s*(\{|\*|[A-Za-z_]\w*)")
INLINE_MOD = re.compile(r"\bmod\s+\w+\s*\{")


def depth_zero(code: str):
    """For each position in code: True where it is outside every brace pair."""
    out, depth = [], 0
    for ch in code:
        if ch == "}":
            depth -= 1
        out.append(depth == 0)
        if ch == "{":
            depth += 1
    return out


def root_names(lib: str, files, full):
    """What a `crate::name` path can name. `lib` is the blanked src/lib.rs.
    Return {"modules": set, "reexports": {name: top module, or None for
    another crate}, "macros": {name: defining module}, "caller_paths": {macro:
    the names its plain `crate::` paths use}}."""
    top = depth_zero(lib)
    modules = set(LAYERS) | {m for m in map(top_module, files) if m is not None}
    reexports = {}
    for m in ROOT_MOD.finditer(lib):
        if top[m.start()]:
            modules.add(m.group(1))
    for m in ROOT_USE.finditer(lib):
        if not top[m.start()]:
            continue
        path = re.sub(r"\s+", "", m.group(1))
        path = re.sub(r"^(?:crate|self)::", "", path)
        first = re.match(r"\w*", path).group(0)
        # The names it brings in: the last segment or the `as` name of each leaf.
        leaves = re.split(r"[{},]", m.group(1))
        for leaf in leaves:
            leaf = leaf.strip()
            name = re.split(r"\s+as\s+", leaf)[-1].split("::")[-1].strip() if leaf else ""
            if name and name not in ("self", "*"):
                reexports[name] = first if first in modules else None
    macros, caller_paths = {}, {}
    for rel in files:
        mod = top_module(rel)
        if mod is None:
            continue
        for m in EXPORTED_MACRO.finditer(full[rel]):
            macros[m.group(1)] = mod
            found = {p.group(1) for p in CALLER_PATH.finditer(braced(full[rel], m.end() - 1))}
            if found:
                caller_paths[m.group(1)] = found
    return {"modules": modules, "reexports": reexports, "macros": macros, "caller_paths": caller_paths}


def resolve(name: str, names):
    """The top module that `crate::name` reaches, CRATE_ROOT for an item of
    src/lib.rs, or None for another crate re-exported there."""
    if name in names["modules"]:
        return name
    if name in names["macros"]:
        return names["macros"][name]
    if name in names["reexports"]:
        return names["reexports"][name]
    return CRATE_ROOT


def module_depth(rel: Path) -> int:
    """How many modules down a file is: 1 for src/io.rs and src/io/mod.rs."""
    return len(rel.parts) - (rel.name == "mod.rs")


def root_paths(code: str, depth: int):
    """Yield (name, line) for each path that starts at the crate root: a
    `crate::` path, and a `super::` path that climbs out of the top-level
    module. `depth` is module_depth of the file; `mod x { }` blocks add one.
    A group gives each of its names; a glob of the crate root gives `*`."""
    def heads(m):
        line = code.count("\n", 0, m.start()) + 1
        if m.group(1) == "{":
            for part in split_top(braced(code, m.end() - 1)):
                head = re.match(r"[A-Za-z_]\w*", part)
                if head and head.group(0) != "self":
                    yield head.group(0), line
        else:
            yield m.group(1), line

    for m in CRATE_PATH.finditer(code):
        yield from heads(m)
    blocks = None
    for m in SUPER_PATH.finditer(code):
        if blocks is None:
            blocks = [(b.end() - 1, b.end() + len(braced(code, b.end() - 1))) for b in INLINE_MOD.finditer(code)]
        nested = sum(1 for start, end in blocks if start < m.start() < end)
        if m.group(0).count("super") >= depth + nested:
            yield from heads(m)


def uses(code: str, depth: int, names):
    """The set of (top module or CRATE_ROOT, line) that a file uses through
    the crate root, with the paths of the exported macros it calls."""
    out = set()
    for name, line in root_paths(code, depth):
        if name == "*":
            out.add((CRATE_ROOT, line))
        elif (target := resolve(name, names)) is not None:
            out.add((target, line))
    for mac, paths in names["caller_paths"].items():
        for m in re.finditer(rf"(?<![\w$]){mac}\s*!", code):
            if re.search(r"macro_rules!\s*$", code[max(0, m.start() - 40):m.start()]):
                continue  # the definition, not a call
            line = code.count("\n", 0, m.start()) + 1
            out.update((t, line) for p in paths if (t := resolve(p, names)) is not None)
    return out


def may_use(src: str, dst) -> bool:
    """Whether crate `src` may use crate `dst` (None: no planned crate)."""
    if src == dst:
        return True
    layer = {name: lay for name, lay, _ in CRATES}
    return dst is not None and (layer[dst] < layer[src] or (src, dst) in MAY_USE)


def readiness(root: Path):
    """For each planned crate: what its production code and its test code use
    that the crate may not. Return {"crates": [{"crate", "layer", "modules",
    "uses": {pair: {file: first_line}}, "test_uses": {...}}], "unplaced":
    the modules of layers 0-5 in no planned crate}."""
    files, full, codes, test_files = read_tree(root)
    lib = root / "src" / "lib.rs"
    lib_code = full.get(Path("lib.rs"), "") if lib.exists() else ""
    names = root_names(lib_code, files, full)
    home = {mod: name for name, _, mods in CRATES for mod in mods}
    found = {name: {"uses": defaultdict(dict), "test_uses": defaultdict(dict)} for name, _, _ in CRATES}
    for rel in files:
        mod = top_module(rel)
        if mod not in home:
            continue
        crate, depth = home[mod], module_depth(rel)
        test_file = named_as_test(rel) or rel in test_files
        prod = set() if test_file else uses(codes[rel], depth, names)
        for kind, hits in (("uses", prod), ("test_uses", uses(full[rel], depth, names) - prod)):
            for target, line in hits:
                if target == mod or (target != CRATE_ROOT and may_use(crate, home.get(target))):
                    continue
                files_of = found[crate][kind][f"{mod} -> {target}"]
                key = f"src/{rel.as_posix()}"
                files_of[key] = min(line, files_of.get(key, line))
    crates = [{"crate": name, "layer": layer, "modules": list(mods),
               **{kind: {pair: dict(sorted(fs.items())) for pair, fs in sorted(found[name][kind].items())}
                  for kind in ("uses", "test_uses")}}
              for name, layer, mods in CRATES]
    unplaced = sorted(m for m, lay in LAYERS.items() if lay <= 5 and m not in home)
    return {"crates": crates, "unplaced": unplaced}


def blocked_files(found) -> int:
    """The number of files behind a crate's `uses` or `test_uses`."""
    return len({f for fs in found.values() for f in fs})


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
        print("A change that adds an upward dependency fails this check. "
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
    return 1 if added or unlayered else 0


def readiness_report(markdown: bool) -> int:
    result = readiness(Path.cwd())

    def status(found, mods):
        n = blocked_files(found)
        return "no modules yet" if not mods else "ready" if not n else f"{n} file{'' if n == 1 else 's'}"

    if markdown:
        print("### Crate readiness\n")
        print("What the modules of each crate planned in `docs/architecture/TARGET.md` use that is "
              "neither in the crate nor in a crate below it.\n")
        print("| Crate | Modules | Production code | Test code |\n| --- | --- | --- | --- |")
        for c in result["crates"]:
            mods = ", ".join(f"`{m}`" for m in c["modules"]) or "—"
            print(f"| `{c['crate']}` | {mods} | {status(c['uses'], c['modules'])} | {status(c['test_uses'], c['modules'])} |")
        for c in result["crates"]:
            for kind, label in (("uses", "production code"), ("test_uses", "test code")):
                if c[kind]:
                    print(f"\n**`{c['crate']}`, {label}**\n")
                    for pair, fs in sorted(c[kind].items(), key=lambda kv: (-len(kv[1]), kv[0])):
                        f, line = next(iter(fs.items()))
                        print(f"- `{pair}`: {len(fs)} file{'' if len(fs) == 1 else 's'}, first `{f}:{line}`")
    else:
        print("Crate readiness (docs/architecture/TARGET.md): uses that are neither in the crate nor below it")
        for c in result["crates"]:
            print(f"  {c['crate']:14s}production: {status(c['uses'], c['modules']):16s}tests: {status(c['test_uses'], c['modules'])}")
            for kind, label in (("uses", ""), ("test_uses", " (tests)")):
                for pair, fs in sorted(c[kind].items(), key=lambda kv: (-len(kv[1]), kv[0])):
                    print(f"      {pair + label:40s}{len(fs):>5}")
    if result["unplaced"]:
        print(f"\nModules in no planned crate (see CRATES): {', '.join(result['unplaced'])}")
    return 0


def main(argv) -> int:
    args = [a for a in argv if a != "--markdown"]
    markdown = "--markdown" in argv
    if args[:1] == ["report"] or not args:
        return report(markdown)
    if args[:1] == ["readiness"] and len(args) == 1:
        return readiness_report(markdown)
    if args[:1] == ["diff"] and len(args) == 2:
        return diff(args[1], markdown)
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
