#!/usr/bin/env python3
"""Generate OpenCADStudio GitHub growth SVG charts."""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path
from xml.sax.saxutils import escape


API_VERSION = "2026-03-10"
WIDTH = 960
HEIGHT = 410
PLOT_LEFT = 72
PLOT_RIGHT = 876
PLOT_TOP = 108
PLOT_BOTTOM = 350


# Isolated historical spike replaced by its neighboring-release mean.
DOWNLOAD_OVERRIDES = {"v0.4.1": 61}


THEMES = {
    "light": {
        "background": "#ffffff",
        "border": "#dbe3ef",
        "grid": "#dbe3ef",
        "text": "#0f172a",
        "muted": "#64748b",
        "line": "#0284c7",
        "area": "#38bdf8",
        "point": "#0369a1",
        "download_line": "#ea580c",
        "download_point": "#c2410c",
    },
    "dark": {
        "background": "#0b1220",
        "border": "#263449",
        "grid": "#263449",
        "text": "#e5edf7",
        "muted": "#94a3b8",
        "line": "#38bdf8",
        "area": "#0ea5e9",
        "point": "#7dd3fc",
        "download_line": "#fb923c",
        "download_point": "#fdba74",
    },
}


def next_link(header: str | None) -> str | None:
    if not header:
        return None
    for item in header.split(","):
        match = re.match(r'\s*<([^>]+)>;\s*rel="([^"]+)"', item)
        if match and match.group(2) == "next":
            return match.group(1)
    return None


def fetch_star_dates(repository: str, token: str | None) -> list[datetime]:
    url = f"https://api.github.com/repos/{repository}/stargazers?per_page=100"
    headers = {
        "Accept": "application/vnd.github.star+json",
        "User-Agent": "OpenCADStudio-star-history",
        "X-GitHub-Api-Version": API_VERSION,
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"

    dates: list[datetime] = []
    for _ in range(100):
        request = urllib.request.Request(url, headers=headers)
        with urllib.request.urlopen(request, timeout=30) as response:
            payload = json.load(response)
            link = response.headers.get("Link")
        for entry in payload:
            value = entry.get("starred_at")
            if value:
                dates.append(datetime.fromisoformat(value.replace("Z", "+00:00")))
        url = next_link(link)
        if not url:
            break
    else:
        raise RuntimeError("stargazer pagination exceeded 100 pages")

    if not dates:
        raise RuntimeError("GitHub returned no dated stargazers")
    dates.sort()
    return dates


def fetch_release_downloads(
    repository: str, token: str | None
) -> list[tuple[str, datetime, int]]:
    url = f"https://api.github.com/repos/{repository}/releases?per_page=100"
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "OpenCADStudio-growth-history",
        "X-GitHub-Api-Version": API_VERSION,
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"

    releases: list[tuple[str, datetime, int]] = []
    for _ in range(100):
        request = urllib.request.Request(url, headers=headers)
        with urllib.request.urlopen(request, timeout=30) as response:
            payload = json.load(response)
            link = response.headers.get("Link")
        for release in payload:
            published_at = release.get("published_at")
            if release.get("draft") or not published_at:
                continue
            tag = str(release.get("tag_name", ""))
            downloads = sum(
                int(asset.get("download_count", 0)) for asset in release.get("assets", [])
            )
            releases.append(
                (
                    tag,
                    datetime.fromisoformat(published_at.replace("Z", "+00:00")),
                    DOWNLOAD_OVERRIDES.get(tag, downloads),
                )
            )
        url = next_link(link)
        if not url:
            break
    else:
        raise RuntimeError("release pagination exceeded 100 pages")

    releases.sort(key=lambda release: release[1])
    if len(releases) < 2:
        raise RuntimeError("GitHub returned fewer than two published releases")
    return releases[:-1]


def nice_axis(value: int) -> tuple[int, int]:
    rough_step = max(value / 5, 1)
    magnitude = 10 ** math.floor(math.log10(rough_step))
    step = magnitude
    for multiplier in (1, 2, 5, 10):
        candidate = multiplier * magnitude
        if candidate >= rough_step:
            step = candidate
            break
    top = math.ceil(value / step) * step
    if top <= value:
        top += step
    return int(top), int(step)


def render_svg(
    repository: str,
    dates: list[datetime],
    releases: list[tuple[str, datetime, int]],
    theme: str,
    chart: str,
    labels: dict | None = None,
) -> str:
    colors = THEMES[theme]
    now = datetime.now(timezone.utc)
    stars = chart == "stars"
    if stars:
        points = [(moment, count) for count, moment in enumerate(dates, 1)]
        title = f"{repository.split('/')[-1]} Stars"
        subtitle = f"{len(dates)} GitHub stars · Updated {now:%d %b %Y}"
        legend = "GitHub stars"
        axis_label = "Stars"
        line_color = colors["line"]
        point_color = colors["point"]
        last_label = f"{len(dates)} stars"
    else:
        points = [(published, downloads) for _, published, downloads in releases]
        total_downloads = sum(downloads for _, _, downloads in releases)
        title = f"{repository.split('/')[-1]} Release Downloads"
        subtitle = (
            f"{total_downloads:,} cleaned asset downloads across {len(releases)} releases "
            f"· Latest excluded · Updated {now:%d %b %Y}"
        )
        legend = "Release downloads"
        axis_label = "Downloads / release"
        line_color = colors["download_line"]
        point_color = colors["download_point"]
        last_label = f"{releases[-1][0]} · {releases[-1][2]}"

    if labels:
        title = labels["stars" if stars else "downloads"]
        legend = title
        axis_label = labels["stars" if stars else "chart_downloads"]
        subtitle = labels["chart_updated"].format(date=now.strftime("%Y-%m-%d"))
        if stars:
            subtitle = f"{len(dates)} · {subtitle}"
            last_label = str(len(dates))
        else:
            subtitle = labels["chart_totals"].format(count=f"{total_downloads:,}", releases=len(releases)) + " · " + subtitle

    start = points[0][0] - timedelta(days=2)
    end = max(now, points[-1][0]) + timedelta(days=2)
    duration = max((end - start).total_seconds(), 1)
    plot_width = PLOT_RIGHT - PLOT_LEFT
    plot_height = PLOT_BOTTOM - PLOT_TOP
    y_top, y_step = nice_axis(max(value for _, value in points))

    def x_position(moment: datetime) -> float:
        ratio = (moment - start).total_seconds() / duration
        return PLOT_LEFT + ratio * plot_width

    def y_position(value: int) -> float:
        return PLOT_BOTTOM - (value / y_top) * plot_height

    if stars:
        line_parts = [f"M {PLOT_LEFT:.1f} {PLOT_BOTTOM:.1f}"]
        for moment, value in points:
            line_parts.append(f"H {x_position(moment):.1f} V {y_position(value):.1f}")
        line_path = " ".join(line_parts)
        area = (
            f'<path d="{line_path} V {PLOT_BOTTOM:.1f} H {PLOT_LEFT:.1f} Z" '
            'fill="url(#series-area)" />'
        )
        point_marks = ""
    else:
        line_path = " ".join(
            ("M" if index == 0 else "L")
            + f" {x_position(moment):.1f} {y_position(value):.1f}"
            for index, (moment, value) in enumerate(points)
        )
        area = ""
        point_marks = "".join(
            f'<circle cx="{x_position(moment):.1f}" cy="{y_position(value):.1f}" '
            f'r="2.5" fill="{point_color}" />'
            for moment, value in points
        )

    grid = []
    for value in range(0, y_top + 1, y_step):
        y = y_position(value)
        grid.append(
            f'<line x1="{PLOT_LEFT}" y1="{y:.1f}" x2="{PLOT_RIGHT}" y2="{y:.1f}" '
            f'stroke="{colors["grid"]}" stroke-width="1" />'
        )
        grid.append(
            f'<text x="{PLOT_LEFT - 14}" y="{y + 4:.1f}" text-anchor="end" '
            f'fill="{colors["muted"]}" font-size="12">{value}</text>'
        )

    x_labels = []
    for index in range(5):
        moment = start + (end - start) * (index / 4)
        x = x_position(moment)
        if labels:
            label = moment.strftime("%Y-%m")
        elif index in (0, 4):
            label = moment.strftime("%b %Y")
        else:
            label = moment.strftime("%b")
        x_labels.append(
            f'<line x1="{x:.1f}" y1="{PLOT_TOP}" x2="{x:.1f}" y2="{PLOT_BOTTOM}" '
            f'stroke="{colors["grid"]}" stroke-width="1" />'
        )
        x_labels.append(
            f'<text x="{x:.1f}" y="{PLOT_BOTTOM + 28}" text-anchor="middle" '
            f'fill="{colors["muted"]}" font-size="12">{escape(label)}</text>'
        )

    last_x = x_position(points[-1][0])
    last_y = y_position(points[-1][1])
    if stars:
        label_x = PLOT_RIGHT
        label_y = max(PLOT_TOP + 16, last_y - 14)
        label_anchor = "end"
    else:
        label_x = last_x - 28
        label_y = min(PLOT_BOTTOM - 8, last_y + 20)
        label_anchor = "end"

    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}" role="img" aria-labelledby="title description">
  <title id="title">{escape(title)}</title>
  <desc id="description">{escape(subtitle)}</desc>
  <defs>
    <linearGradient id="series-area" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="{colors["area"]}" stop-opacity="0.34" />
      <stop offset="100%" stop-color="{colors["area"]}" stop-opacity="0.03" />
    </linearGradient>
  </defs>
  <rect x="0.5" y="0.5" width="{WIDTH - 1}" height="{HEIGHT - 1}" rx="14" fill="{colors["background"]}" stroke="{colors["border"]}" />
  <g font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif">
    <text x="{PLOT_LEFT}" y="34" fill="{colors["text"]}" font-size="21" font-weight="700">{escape(title)}</text>
    <text x="{PLOT_LEFT}" y="58" fill="{colors["muted"]}" font-size="13">{escape(subtitle)}</text>
    <line x1="{PLOT_LEFT}" y1="82" x2="{PLOT_LEFT + 24}" y2="82" stroke="{line_color}" stroke-width="3" />
    <text x="{PLOT_LEFT + 32}" y="86" fill="{colors["text"]}" font-size="12">{escape(legend)}</text>
    <text x="{PLOT_LEFT}" y="{PLOT_TOP - 8}" fill="{line_color}" font-size="12">{escape(axis_label)}</text>
    {''.join(grid)}
    {''.join(x_labels)}
    {area}
    <path d="{line_path}" fill="none" stroke="{line_color}" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" />
    {point_marks}
    <circle cx="{last_x:.1f}" cy="{last_y:.1f}" r="5" fill="{point_color}" stroke="{colors["background"]}" stroke-width="3" />
    <text x="{label_x:.1f}" y="{label_y:.1f}" text-anchor="{label_anchor}" fill="{line_color}" font-size="14" font-weight="700">{escape(last_label)}</text>
  </g>
</svg>
'''


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--repository",
        default=os.environ.get("GITHUB_REPOSITORY", "HakanSeven12/OpenCADStudio"),
    )
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    args = parser.parse_args()
    catalogs = {path.stem: json.loads(path.read_text()) for path in (Path(__file__).resolve().parents[1] / "site/locales").glob("*.json")}

    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    dates = fetch_star_dates(args.repository, token)
    releases = fetch_release_downloads(args.repository, token)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for theme in THEMES:
        for chart in ("stars", "downloads"):
            output = args.output_dir / f"{chart[:-1]}-history-{theme}.svg"
            output.write_text(
                render_svg(args.repository, dates, releases, theme, chart),
                encoding="utf-8",
            )
            for locale, labels in catalogs.items():
                localized = args.output_dir / locale / output.name
                localized.parent.mkdir(parents=True, exist_ok=True)
                localized.write_text(render_svg(args.repository, dates, releases, theme, chart, labels), encoding="utf-8")
    print(
        f"growth history: {len(dates)} stars, "
        f"{sum(downloads for _, _, downloads in releases)} downloads"
    )


if __name__ == "__main__":
    main()
