#!/usr/bin/env python3
"""Prepare weekly releases and share package versions across build targets."""

import argparse
from datetime import date, datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
try:
    import tomllib
except ImportError:  # Python < 3.11 (e.g. ubuntu-22.04)
    try:
        import tomli as tomllib
    except ImportError:
        tomllib = None


def run(*args):
    return subprocess.check_output(args, text=True, encoding="utf-8").rstrip("\n")


def gh(*args):
    return json.loads(run("gh", *args))


def versions(tag):
    version = tag.removeprefix("v")
    # `2026.40` is the weekly release; `2026.40.1` a fix release on top of it.
    weekly = re.fullmatch(r"(20\d{2})\.(\d{2})(?:\.([1-9]\d*))?", version)
    if weekly:
        year, week = int(weekly[1]), int(weekly[2])
        patch = int(weekly[3] or 0)
        date.fromisocalendar(year, week, 1)
        cargo = f"{year}.{week}.{patch}"
        msi = f"{year - 2000}.{week}.{patch}"
    elif re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version):
        cargo = msi = version
        major, minor, patch = map(int, version.split("."))
        if major > 255 or minor > 255 or patch > 65535:
            raise ValueError(f"Unsupported MSI version: {version}")
    else:
        raise ValueError(f"Invalid release version: {tag}")
    return {"version": version, "cargo": cargo, "msi": msi, "tag": f"v{version}"}


def cargo_version():
    return re.search(r'^version = "([^"]+)"', Path("Cargo.toml").read_text(encoding="utf-8"), re.M)[1]


def display_version(cargo):
    year, week, patch = cargo.split(".")
    if len(year) == 4 and year.startswith("20"):
        weekly = f"{year}.{int(week):02}"
        return versions(weekly if patch == "0" else f"{weekly}.{patch}")["version"]
    return cargo


def output(values):
    lines = "".join(f"{key}={value}\n" for key, value in values.items())
    print(lines, end="")
    if path := os.environ.get("GITHUB_OUTPUT"):
        with open(path, "a", encoding="utf-8") as stream:
            stream.write(lines)


def release_notes(previous, tag, repo, head="HEAD"):
    sections = {
        "Modeling, drawing and annotation": [],
        "Rendering, plotting and file fidelity": [],
        "Interface, web and plugins": [],
        "Maintenance": [],
    }
    entries = run("git", "log", "--first-parent", "--format=%s%x1f%b%x1e", f"{previous}..{head}")
    for entry in entries.split("\x1e"):
        if "\x1f" not in entry:
            continue
        subject, body = entry.strip("\n").split("\x1f", 1)
        if subject.startswith("Merge pull request"):
            subject = next((line for line in body.splitlines() if line.strip()), subject)
        if subject.startswith(("Merge ", "Release v")):
            continue
        subject = re.sub(r"^(?:feat|fix|perf|chore|docs)(?:\([^)]*\))?:\s*", "", subject)
        text = subject.lower()
        if re.search(r"render|plot|print|gpu|buffer|save|file|serializ|vram", text):
            section = "Rendering, plotting and file fidelity"
        elif re.search(r"ui|web|plugin|ribbon|theme|translat|dropdown|mcp|control|cli", text):
            section = "Interface, web and plugins"
        elif re.search(r"kernel|curve|solid|surface|extrud|loft|sweep|revol|presspull|snap|hatch|grip|trim|offset|draw|dimension", text):
            section = "Modeling, drawing and annotation"
        else:
            section = "Maintenance"
        if subject not in sections[section]:
            sections[section].append(subject)
    lines = ["## Highlights", "", f"- Weekly {tag.removeprefix('v')} release for web and desktop from the same source revision.", ""]
    for title, subjects in sections.items():
        if subjects:
            lines += [f"## {title}", ""]
            # ponytail: six recent entries per section; full history is linked below.
            lines += [f"- {subject}" for subject in subjects[:6]]
            lines += [""]
    lines += [f"**Full Changelog:** https://github.com/{repo}/compare/{previous}...{tag}", ""]
    return "\n".join(lines)


UNRELEASED = Path("changelogs/unreleased.md")
EMPTY_SUMMARY = "> **Summary:** Nothing yet."


def changelog_page(tag, day):
    """changelogs/unreleased.md titled for the release: published verbatim as
    the release notes and kept as changelogs/<tag>.md. None when empty."""
    if not UNRELEASED.is_file():
        return None
    body = UNRELEASED.read_text(encoding="utf-8").partition("\n")[2]
    if body.strip() in ("", EMPTY_SUMMARY):
        return None
    return f"# OpenCADStudio - Release {tag} ({day})\n{body}"


def rotate_changelog(tag, day, page):
    """Keep the release page as changelogs/<tag>.md and start a new unreleased
    page; CHANGELOG.md's index row and summary follow."""
    UNRELEASED.with_name(f"{tag}.md").write_text(page, encoding="utf-8")
    UNRELEASED.write_text(f"# OpenCADStudio - Unreleased ({day} to Present)\n\n{EMPTY_SUMMARY}\n", encoding="utf-8")
    index = Path("CHANGELOG.md")
    text = index.read_text(encoding="utf-8")
    row = re.search(r"^\| \*\*\[Unreleased\]\(changelogs/unreleased\.md\)\*\* \| [^|]* \| ([^|]*) \|.*$", text, re.M)
    section = re.search(r"^## \[Unreleased\] - .*$", text, re.M)
    if not row or not section:
        raise ValueError("CHANGELOG.md has no Unreleased row or section")
    new_row = (f"| **[Unreleased](changelogs/unreleased.md)** | *{day} to Present* | — | [changelogs/unreleased.md](changelogs/unreleased.md) |\n"
               f"| **[{tag}](changelogs/{tag}.md)** | *{day}* | {row[1]} | [changelogs/{tag}.md](changelogs/{tag}.md) |")
    new_section = (f"## [Unreleased] - {day}\n\n> **Summary:** Nothing yet. See the [full Unreleased notes](changelogs/unreleased.md).\n\n"
                   f"## [{tag}] - {day}")
    # The old Unreleased summary now links the release page.
    end = text.find("\n## ", section.end())
    end = len(text) if end < 0 else end
    old = text[section.end():end].replace("[full Unreleased notes](changelogs/unreleased.md)", f"[full {tag} release notes](changelogs/{tag}.md)")
    text = text[:row.start()] + new_row + text[row.end():section.start()] + new_section + old + text[end:]
    index.write_text(text, encoding="utf-8")


def update_cargo_toml_version(content, old, new):
    header = re.search(r"^\[package\]\s*$", content, re.M)
    if not header:
        raise ValueError("Missing [package] section in Cargo.toml")
    next_header = re.search(r"^\[", content[header.end():], re.M)
    sec_end = header.end() + next_header.start() if next_header else len(content)
    section = content[header.end():sec_end]
    matches = list(re.finditer(r'^(version\s*=\s*)"([^"]+)"(.*)$', section, re.M))
    if len(matches) != 1 or matches[0].group(2) != old:
        raise ValueError(f"Expected package version {old!r} in Cargo.toml [package] section")
    m = matches[0]
    updated_section = (
        section[:m.start()]
        + f'{m.group(1)}"{new}"{m.group(3)}'
        + section[m.end():]
    )
    updated = content[:header.end()] + updated_section + content[sec_end:]
    if tomllib is not None and tomllib.loads(updated)["package"]["version"] != new:
        raise ValueError("Updated Cargo.toml failed TOML version verification")
    return updated


def update_cargo_lock_version(content, package_name, old, new):
    headers = list(re.finditer(r"^\[\[package\]\]\s*$", content, re.M))
    if not headers:
        raise ValueError("Missing [[package]] entries in Cargo.lock")
    target_bounds = None
    for idx, header in enumerate(headers):
        end = headers[idx + 1].start() if idx + 1 < len(headers) else len(content)
        block = content[header.end():end]
        if re.search(rf'^name\s*=\s*"{re.escape(package_name)}"\s*$', block, re.M):
            if target_bounds is not None:
                raise ValueError(f"Multiple [[package]] entries for {package_name} in Cargo.lock")
            target_bounds = (header.end(), end)
    if target_bounds is None:
        raise ValueError(f"Missing [[package]] entry for {package_name} in Cargo.lock")
    start, end = target_bounds
    block = content[start:end]
    matches = list(re.finditer(r'^(version\s*=\s*)"([^"]+)"(.*)$', block, re.M))
    if len(matches) != 1 or matches[0].group(2) != old:
        raise ValueError(f"Expected {package_name} version {old!r} in Cargo.lock")
    m = matches[0]
    updated_block = (
        block[:m.start()]
        + f'{m.group(1)}"{new}"{m.group(3)}'
        + block[m.end():]
    )
    updated = content[:start] + updated_block + content[end:]
    if tomllib is not None:
        parsed = tomllib.loads(updated)
        found = [pkg for pkg in parsed.get("package", []) if pkg.get("name") == package_name]
        if len(found) != 1 or found[0].get("version") != new:
            raise ValueError(f"Updated Cargo.lock failed TOML version verification for {package_name}")
    return updated


def sync_homebrew(tag, arm_sha, intel_sha, cask_path=Path("packaging/homebrew/open-cad-studio.rb")):
    version = versions(tag)["version"]
    for label, digest in (("arm", arm_sha), ("intel", intel_sha)):
        if not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ValueError(f"Invalid {label} SHA-256 digest: {digest!r}")
    cask_path = Path(cask_path)
    text = cask_path.read_text(encoding="utf-8")
    text, v_count = re.subn(r'^(\s*version\s+")[^"]+(")', rf'\g<1>{version}\g<2>', text, count=1, flags=re.M)
    text, arm_count = re.subn(r'^(\s*sha256\s+arm:\s*")[^"]+(")', rf'\g<1>{arm_sha}\g<2>', text, count=1, flags=re.M)
    text, intel_count = re.subn(r'^(\s*intel:\s*")[^"]+(")', rf'\g<1>{intel_sha}\g<2>', text, count=1, flags=re.M)
    if (v_count, arm_count, intel_count) != (1, 1, 1):
        raise ValueError(f"Could not update Homebrew cask fields cleanly in {cask_path}")
    cask_path.write_text(text, encoding="utf-8")
    return version


def prepare(publish):
    if run("git", "status", "--porcelain"):
        raise ValueError("Release preparation requires a clean working tree")
    repo = os.environ["GITHUB_REPOSITORY"]
    current = datetime.now(timezone.utc)
    tag = current.strftime("v%G.%V")
    version = versions(tag)
    latest = gh("release", "view", "--json", "tagName")["tagName"]
    if publish and os.environ.get("GITHUB_REF") != "refs/heads/main":
        raise ValueError("Weekly releases must run on main")
    existing = run("git", "tag", "--list", tag)
    if existing:
        # A retry always uses the original tag, even when main has advanced.
        sha = run("git", "rev-parse", f"{tag}^{{commit}}")
        releases = gh("release", "list", "--limit", "100", "--json", "tagName,isDraft")
        if any(release["tagName"] == tag for release in releases):
            published = gh("release", "view", tag, "--json", "name,isDraft,body")
            if published["isDraft"] or not published["body"].strip() or published["name"] != version["version"]:
                raise ValueError("Existing weekly release metadata is invalid")
            output({"ready": str(publish).lower(), "tag": tag, "commit": sha})
            return
    elif run("git", "rev-list", "--count", f"{latest}..HEAD") == "0":
        output({"ready": "false"})
        return
    day = current.strftime("%Y-%m-%d")
    if existing:
        # A retry publishes the page the release commit already keeps.
        try:
            page = run("git", "show", f"{tag}:changelogs/{tag}.md") + "\n"
        except subprocess.CalledProcessError:
            page = None
    else:
        page = changelog_page(tag, day)
    notes = page or release_notes(latest, tag, repo, tag if existing else "HEAD")
    print(notes)
    if not publish:
        output({"ready": "false", **version, "commit": run("git", "rev-parse", "HEAD")})
        return
    if not existing:
        old = cargo_version()
        cargo_toml_path = Path("Cargo.toml")
        cargo_lock_path = Path("Cargo.lock")
        updates = {
            cargo_toml_path: update_cargo_toml_version(
                cargo_toml_path.read_text(encoding="utf-8"), old, version["cargo"]
            ),
            cargo_lock_path: update_cargo_lock_version(
                cargo_lock_path.read_text(encoding="utf-8"), "OpenCADStudio", old, version["cargo"]
            ),
        }
        for path, content in updates.items():
            path.write_text(content, encoding="utf-8")
        run("git", "config", "user.name", "github-actions[bot]")
        run("git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
        run("git", "add", "Cargo.toml", "Cargo.lock")
        if page:
            rotate_changelog(tag, day, page)
            run("git", "add", "changelogs", "CHANGELOG.md")
        run("git", "commit", "-m", f"Release {tag}")
        run("git", "tag", "-a", tag, "-m", f"Release {tag}")
        run("git", "push", "--atomic", "origin", "HEAD:main", f"refs/tags/{tag}")
        sha = run("git", "rev-parse", "HEAD")
    with tempfile.NamedTemporaryFile(mode="w", suffix=".md", encoding="utf-8") as notes_file:
        notes_file.write(notes)
        notes_file.flush()
        run("gh", "release", "create", tag, "--verify-tag", "--title", version["version"], "--notes-file", notes_file.name, "--latest")
    published = gh("release", "view", tag, "--json", "name,body,isDraft")
    if published != {"name": version["version"], "body": notes, "isDraft": False}:
        raise ValueError("Published release notes do not match the prepared release")
    output({"ready": "true", "tag": tag, "commit": sha})


def verify_native(tag):
    release = gh("release", "view", tag, "--json", "assets,body,isDraft")
    expected = {
        f"OpenCADStudio-{tag}-{suffix}"
        for suffix in ("linux-x86_64.AppImage", "linux-aarch64.AppImage", "linux-x86_64.snap",
                       "windows-x86_64-portable.exe", "windows-x86_64-installer.msi",
                       "windows-arm64-portable.exe", "windows-arm64-installer.msi",
                       "macos-arm64.dmg", "macos-x86_64.dmg")
    }
    uploaded = {asset["name"] for asset in release["assets"] if asset["size"] > 0 and asset["state"] == "uploaded"}
    if release["isDraft"] or not release["body"].strip() or not expected <= uploaded:
        raise ValueError(f"Incomplete native release: {sorted(expected - uploaded)}")
    print(f"Verified native release {tag}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare_parser = commands.add_parser("prepare")
    prepare_parser.add_argument("--publish", action="store_true")
    version_parser = commands.add_parser("versions")
    version_parser.add_argument("--tag", default="")
    verify_parser = commands.add_parser("verify-native")
    verify_parser.add_argument("--tag", required=True)
    homebrew_parser = commands.add_parser("sync-homebrew")
    homebrew_parser.add_argument("--tag", required=True)
    homebrew_parser.add_argument("--arm-sha", required=True)
    homebrew_parser.add_argument("--intel-sha", required=True)
    homebrew_parser.add_argument("--cask", default="packaging/homebrew/open-cad-studio.rb")
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.publish)
    elif args.command == "verify-native":
        verify_native(args.tag)
    elif args.command == "sync-homebrew":
        version = sync_homebrew(args.tag, args.arm_sha, args.intel_sha, args.cask)
        output({"version": version, "cask": str(args.cask)})
    else:
        version = versions(args.tag or display_version(cargo_version()))
        if args.tag and version["cargo"] != cargo_version():
            raise ValueError("Release tag and Cargo version do not match")
        output(version)


if __name__ == "__main__":
    main()
