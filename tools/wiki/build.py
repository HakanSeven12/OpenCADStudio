#!/usr/bin/env python3
"""Build the GitHub wiki from the source tree.

    python3 tools/wiki/build.py OUT_DIR

Everything written by hand lives in docs/wiki/:

    pages/*.md            topic pages, copied as they are
    commands/NAME.md      one command page body (front matter + Markdown)
    sysvars/NAME.md       one system variable page body
    images/**             screenshots and diagrams
    since.json            the release each command / variable first shipped in

Everything else is read from the code, so the reference follows the
registry on every release: the registered command names (`CommandRegistration`),
the system variables (`SYSTEM_VARIABLES`), the default aliases (assets/ocad.pgp),
the default keyboard shortcuts (src/app/shortcuts.rs) and the ribbon icons.

Front matter is a few `key: value` lines between `---` lines:

    ---
    summary: Creates straight line segments.
    related: PLINE, ARC, XLINE
    web: Not available in the web app.
    ---

OUT_DIR is the wiki checkout; generated pages and copied images replace what
is there, other files are left alone.
"""

import json
import os
import re
import shutil
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(ROOT, "src")
WIKI = os.path.join(ROOT, "docs", "wiki")
ICONS = os.path.join(ROOT, "assets", "icons")


def read(path):
    with open(path, encoding="utf-8", errors="ignore") as f:
        return f.read()


def rust_sources():
    for base, _, files in os.walk(SRC):
        for name in files:
            if name.endswith(".rs"):
                yield os.path.join(base, name)


def registry():
    """Registered command names and the system variables among them."""
    names = set()
    icons = {}
    for path in rust_sources():
        text = read(path)
        for m in re.finditer(r"CommandRegistration\s*\{\s*names:\s*&\[([^\]]*)\]", text):
            names.update(re.findall(r'"([A-Z0-9_-]+)"', m.group(1)))
        for m in re.finditer(r'id:\s*"([A-Z0-9_-]+)"[^}]{0,400}?assets/icons/([\w/.-]+\.svg)', text, re.S):
            icons.setdefault(m.group(1), m.group(2))
    reg = read(os.path.join(SRC, "modules", "registry.rs"))
    start = reg.index("const SYSTEM_VARIABLES")
    sysvars = set(re.findall(r'"([A-Z0-9_]+)"', reg[start:reg.index("];", start)]))
    icon_alias = dict(
        re.findall(r'\(\s*"([A-Z0-9_-]+)",\s*"([A-Z0-9_-]+)"\s*\)', reg[: reg.index("pub fn command_icon")])
    )
    return names, sysvars, icons, icon_alias


def aliases():
    """command -> [aliases] from the default alias file."""
    out = {}
    for line in read(os.path.join(ROOT, "assets", "ocad.pgp")).splitlines():
        m = re.match(r"\s*([^;,\s]+)\s*,\s*\*(\S+)", line)
        if m:
            out.setdefault(m.group(2).upper(), []).append(m.group(1).upper())
    return out


def shortcuts():
    """command -> [keys] from the default key bindings."""
    text = read(os.path.join(SRC, "app", "shortcuts.rs"))
    body = text[text.index("fn default_bindings") :]
    body = body[: body.index(".into_iter()")]
    pattern = r'\(\s*(?:"([^"]+)"\.to_string\(\)|format!\("([^"]+)"\))\s*,\s*"([A-Z0-9_]+)"\s*\)'
    out = {}
    for plain, formatted, cmd in re.findall(pattern, body):
        key = (plain or formatted).replace("{ACCEL}", "CTRL")
        key = "+".join(part if len(part) <= 3 and part[0] == "F" else part.title() for part in key.split("+"))
        out.setdefault(cmd, []).append(key)
    return out


def front_matter(path):
    """(fields, body) of a page source; missing file -> ({}, "")."""
    if not os.path.exists(path):
        return {}, ""
    text = read(path)
    fields = {}
    if text.startswith("---\n"):
        end = text.index("\n---", 4)
        for line in text[4:end].splitlines():
            if ":" in line:
                key, value = line.split(":", 1)
                fields[key.strip()] = value.strip()
        text = text[end + 4 :].lstrip("\n")
    return fields, text


def version():
    m = re.search(r'^version\s*=\s*"([^"]+)"', read(os.path.join(ROOT, "Cargo.toml")), re.M)
    return m.group(1) if m else "unknown"


def main(out):
    names, sysvars, icon_ids, icon_alias = registry()
    alias_of = aliases()
    alias_target = {a: c for c, al in alias_of.items() for a in al}
    keys = shortcuts()
    since_path = os.path.join(WIKI, "since.json")
    since = json.loads(read(since_path)) if os.path.exists(since_path) else {}
    current = version()
    used_icons = set()

    def icon(name):
        seen = set()
        while name and name not in seen:
            seen.add(name)
            if name in icon_ids:
                return icon_ids[name]
            name = icon_alias.get(name) or (name[1:] if name.startswith("-") else None)
        return None

    def img(path, size=20):
        used_icons.add(path)
        return f'<img src="images/icons/{path}" width="{size}" alt="">'

    def page_of(name):
        name = name.lstrip("-").split("_")[0] if "_" in name else name.lstrip("-")
        if name in sysvars:
            return f"{name}-System-Variable"
        return f"{name}-Command"

    def link(name):
        return f"[`{name}`]({page_of(name)})"

    def first_release(name):
        if name not in since:
            since[name] = current
        return since[name]

    commands = {}
    for name in names:
        if "_" in name or name in sysvars:
            continue
        base = name.lstrip("-")
        commands.setdefault(base, set()).add(name)

    # Command pages.
    index = {}
    for base in sorted(commands):
        forms = sorted(commands[base], key=lambda n: (n.startswith("-"), n))
        fields, body = front_matter(os.path.join(WIKI, "commands", f"{base}.md"))
        target = alias_target.get(base)
        if target and target != base and not fields:
            fields = {"summary": f"Same as {link(target)}."}
        summary = fields.get("summary", "")
        index[base] = summary
        ic = icon(base)
        lines = [f"# {base} (Command)", ""]
        if ic:
            lines += [img(ic, 32), ""]
        if summary:
            lines += [summary, ""]
        facts = [("Command", " · ".join(f"`{f}`" for f in forms))]
        al = sorted(set(alias_of.get(base, [])) - {base})
        if al:
            facts.append(("Aliases", " · ".join(f"`{a}`" for a in al)))
        if keys.get(base):
            facts.append(("Shortcut", " · ".join(keys[base])))
        if fields.get("ribbon"):
            facts.append(("Ribbon", fields["ribbon"]))
        facts.append(("Web app", fields.get("web", "Available")))
        facts.append(("Since", first_release(base)))
        lines += ["| | |", "|---|---|"] + [f"| **{k}** | {v} |" for k, v in facts] + [""]
        if body:
            lines += [body.rstrip(), ""]
        if fields.get("related"):
            related = [r.strip() for r in fields["related"].split(",") if r.strip()]
            lines += ["## Related", "", " · ".join(link(r) for r in related), ""]
        lines += ["---", "[Command Reference](Command-Reference) · [System Variables](System-Variables)"]
        write(out, f"{base}-Command.md", "\n".join(lines) + "\n")

    # System variable pages.
    sv_index = {}
    for name in sorted(sysvars):
        fields, body = front_matter(os.path.join(WIKI, "sysvars", f"{name}.md"))
        summary = fields.get("summary", "")
        sv_index[name] = (fields, summary)
        lines = [f"# {name} (System Variable)", ""]
        if summary:
            lines += [summary, ""]
        facts = [
            ("Type", fields.get("type", "—")),
            ("Saved in", fields.get("saved", "—")),
            ("Default", fields.get("default", "—")),
            ("Since", first_release(name)),
        ]
        lines += ["| | |", "|---|---|"] + [f"| **{k}** | {v} |" for k, v in facts] + [""]
        if body:
            lines += [body.rstrip(), ""]
        if fields.get("related"):
            related = [r.strip() for r in fields["related"].split(",") if r.strip()]
            lines += ["## Related", "", " · ".join(link(r) for r in related), ""]
        lines += ["---", "[System Variables](System-Variables) · [Command Reference](Command-Reference)"]
        write(out, f"{name}-System-Variable.md", "\n".join(lines) + "\n")

    # Indexes.
    letters = sorted({b[0] for b in index})
    lines = [
        "# Command Reference",
        "",
        "Every command, in alphabetical order. Each name opens the command's own page; "
        "**F1** inside a running command opens the same page. A command that starts with `-` "
        "is the command-line form of a dialog command and is described on the command's page.",
        "",
        "Aliases are the short names from the default alias file; see "
        "[Customization](Customization#command-aliases) to change them. Type an apostrophe "
        "before a command to run it inside another one, for example `'ZOOM`.",
        "",
        "Jump to: " + " · ".join(f"[{c}](#{c.lower()})" for c in letters),
    ]
    current_letter = None
    for base in sorted(index):
        if base[0] != current_letter:
            current_letter = base[0]
            lines += ["", f"## {current_letter}", "", "| | Command | Aliases | What it does |", "|---|---|---|---|"]
        ic = icon(base)
        al = ", ".join(f"`{a}`" for a in sorted(set(alias_of.get(base, [])) - {base})) or "—"
        lines.append(f"| {img(ic) if ic else ''} | {link(base)} | {al} | {index[base] or '—'} |")
    write(out, "Command-Reference.md", "\n".join(lines) + "\n")

    lines = [
        "# System Variables",
        "",
        "System variables hold settings for the drawing and for the program. Type a variable's "
        "name at the command line to see its value and enter a new one, or use `SETVAR`. "
        "Each name opens the variable's own page.",
        "",
        "| Variable | Default | What it controls |",
        "|---|---|---|",
    ]
    for name in sorted(sv_index):
        fields, summary = sv_index[name]
        lines.append(f"| {link(name)} | {fields.get('default', '—')} | {summary or '—'} |")
    write(out, "System-Variables.md", "\n".join(lines) + "\n")

    # What's new: every release's summary, newest first, from changelogs/.
    notes = []
    folder = os.path.join(ROOT, "changelogs")
    for name in os.listdir(folder):
        m = re.match(r"v([\d.]+)\.md$", name)
        if not m:
            continue
        text = read(os.path.join(folder, name))
        title = re.search(r"^# .*?\(([^)]+)\)", text, re.M)
        summary = re.search(r"^> \*\*Summary:\*\*\s*(.+)$", text, re.M)
        notes.append((tuple(int(p) for p in m.group(1).split(".")), m.group(1), title.group(1) if title else "", summary.group(1).strip() if summary else ""))
    lines = [
        "# What's New",
        "",
        "Each release in short, newest first. The full notes, with every change and its contributors, "
        "are in the linked release notes; `CHANGELOG` opens the releases page.",
    ]
    for _, ver, date, summary in sorted(notes, reverse=True):
        url = f"https://github.com/HakanSeven12/OpenCADStudio/blob/main/changelogs/v{ver}.md"
        lines += ["", f"## {ver}" + (f" ({date})" if date else ""), "", summary or "—", "", f"[Release notes]({url})"]
    write(out, "Whats-New.md", "\n".join(lines) + "\n")

    # Hand-written pages and images.
    for name in os.listdir(os.path.join(WIKI, "pages")):
        text = read(os.path.join(WIKI, "pages", name))
        used_icons.update(re.findall(r'src="images/icons/([^"]+)"', text))
        write(out, name, text)
    images = os.path.join(WIKI, "images")
    for base, _, files in os.walk(images):
        for name in files:
            src = os.path.join(base, name)
            dst = os.path.join(out, "images", os.path.relpath(src, images))
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.copyfile(src, dst)
    for path in used_icons:
        dst = os.path.join(out, "images", "icons", path)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copyfile(os.path.join(ICONS, path), dst)
    shutil.copyfile(os.path.join(ROOT, "assets", "logo.svg"), os.path.join(out, "images", "logo.svg"))

    fix_links(out)
    with open(since_path, "w", encoding="utf-8") as f:
        json.dump(dict(sorted(since.items())), f, indent=0)
        f.write("\n")
    print(f"{len(index)} commands, {len(sv_index)} system variables, {len(used_icons)} icons")


def write(out, name, text):
    with open(os.path.join(out, name), "w", encoding="utf-8") as f:
        f.write(text)


def fix_links(out):
    """Point `NAME-Command` / `NAME-System-Variable` links at the page that
    exists (a name can be a system variable, a command or neither); a link to
    neither becomes plain code text."""
    pages = {os.path.splitext(n)[0] for n in os.listdir(out) if n.endswith(".md")}

    def repl(m):
        label, name = m.group(1), m.group(2)
        for page in (f"{name}-Command", f"{name}-System-Variable"):
            if page in pages:
                return f"[{label}]({page}{m.group(4) or ''})"
        return label

    pattern = re.compile(r"\[([^\]]+)\]\(([A-Z0-9_-]+?)-(Command|System-Variable)(#[^)]*)?\)")
    for name in pages:
        path = os.path.join(out, f"{name}.md")
        text = read(path)
        fixed = pattern.sub(repl, text)
        if fixed != text:
            write(out, f"{name}.md", fixed)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
