#!/usr/bin/env python3
"""Build the static website using the application's supported locales."""

import hashlib
import json
import re
import shutil
import sys
from html import escape
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BASE = "https://www.opencadstudio.com"
REPO = "https://github.com/HakanSeven12/OpenCADStudio"


def locale_path(locale):
    return f"/{locale}/"


def load_catalogs():
    catalogs = {p.stem: json.loads(p.read_text()) for p in sorted((ROOT / "site/locales").glob("*.json"))}
    supported = {p.parent.name for p in (ROOT / "locales").glob("*/opencadstudio.ftl")}
    if catalogs.keys() != supported:
        raise ValueError(f"Website locale mismatch: {catalogs.keys() ^ supported}")
    source = catalogs["en-US"]
    for locale, messages in catalogs.items():
        if messages.keys() != source.keys():
            raise ValueError(f"{locale}: translation key mismatch: {messages.keys() ^ source.keys()}")
        for key, value in messages.items():
            if not isinstance(value, str) or not value.strip():
                raise ValueError(f"{locale}: empty translation for {key}")
            if set(re.findall(r"\{\w+\}", value)) != set(re.findall(r"\{\w+\}", source[key])):
                raise ValueError(f"{locale}: placeholder mismatch for {key}")
    return catalogs


def build(output):
    catalogs = load_catalogs()
    template = (ROOT / "index.html").read_text()
    (output / "assets").mkdir(parents=True, exist_ok=True)

    def asset(source):
        fingerprint = hashlib.sha256(source.read_bytes()).hexdigest()[:12]
        path = f"/assets/{source.stem}-{fingerprint}{source.suffix}"
        shutil.copyfile(source, output / path.lstrip("/"))
        return path

    logo = asset(ROOT / "assets/logo.svg")
    shutil.copyfile(ROOT / "assets/logo.svg", output / "assets/logo.svg")
    ico = asset(ROOT / "site/favicon.ico")
    touch = asset(ROOT / "site/apple-touch-icon.png")
    icons = (f'<link rel="icon" href="{ico}" sizes="16x16 32x32 48x48" />\n'
             f'<link rel="icon" href="{logo}" type="image/svg+xml" sizes="any" />\n'
             f'<link rel="apple-touch-icon" href="{touch}" sizes="180x180" />')
    manifest = json.loads((ROOT / "site/site.webmanifest").read_text())
    manifest["icons"] = [{"src": logo, "sizes": "any", "type": "image/svg+xml"}]
    for size in (192, 512):
        manifest["icons"].append({"src": asset(ROOT / f"site/icon-{size}.png"), "sizes": f"{size}x{size}", "type": "image/png"})
    shutil.copyfile(ROOT / "site/favicon.ico", output / "favicon.ico")
    shutil.copyfile(ROOT / "site/apple-touch-icon.png", output / "apple-touch-icon.png")
    css_version = hashlib.sha256((ROOT / "site/site.css").read_bytes()).hexdigest()[:12]
    script = "const SITE_LOCALES = " + json.dumps(list(catalogs)) + ";\n" + (ROOT / "site/site.js").read_text()
    (output / "site.js").write_text(script, encoding="utf-8")
    js_version = hashlib.sha256(script.encode()).hexdigest()[:12]
    alternates = '\n'.join(f'<link rel="alternate" hreflang="{lang}" href="{BASE}{locale_path(lang)}" />' for lang in catalogs)
    alternates += f'\n<link rel="alternate" hreflang="x-default" href="{BASE}/" />'
    for locale, messages in catalogs.items():
        path = locale_path(locale)
        destination = output / path.lstrip("/")
        destination.mkdir(parents=True, exist_ok=True)
        language_links = '\n'.join(
            f'<a href="{locale_path(lang)}" lang="{lang}" hreflang="{lang}" dir="auto"'
            + (' aria-current="page"' if lang == locale else '')
            + f'>{escape(labels["name"])}</a>' for lang, labels in catalogs.items()
        )
        schema = {
            "@context": "https://schema.org", "@type": "SoftwareApplication",
            "name": "Open CAD Studio", "url": BASE + path, "inLanguage": locale,
            "description": messages["description"], "applicationCategory": "DesignApplication",
            "operatingSystem": "Windows, Linux, macOS, Web", "isAccessibleForFree": True,
            "license": "https://www.gnu.org/licenses/gpl-3.0.html", "codeRepository": REPO,
            "downloadUrl": REPO + "/releases/latest", "installUrl": BASE + "/app/",
            "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
        }
        values = {key: escape(value) for key, value in messages.items()}
        values.update(locale=locale, direction="rtl" if locale == "ar-SA" else "ltr",
                      path=path, url=BASE + path, og_locale=locale.replace("-", "_"),
                      logo=logo, icons=icons, css_version=css_version, js_version=js_version, alternates=alternates,
                      languages=escape(messages["languages"].format(count=len(catalogs))),
                      language_links=language_links,
                      schema=json.dumps(schema, ensure_ascii=False).replace("<", "\\u003c"))
        rendered = re.sub(r"\{\{(\w+)\}\}", lambda match: values[match[1]], template)
        (destination / "index.html").write_text(rendered, encoding="utf-8")
        localized_manifest = {**manifest, "lang": locale, "dir": values["direction"],
                              "description": messages["description"], "start_url": path,
                              "shortcuts": [{"name": messages["launch"], "url": "/app/"}]}
        (destination / "site.webmanifest").write_text(json.dumps(localized_manifest, ensure_ascii=False, indent=2) + '\n', encoding="utf-8")
    for filename in ("index.html", "site.webmanifest"):
        shutil.copyfile(output / "en-US" / filename, output / filename)
    urls = f'<url><loc>{BASE}/</loc></url>\n' + ''.join(f'<url><loc>{BASE}{locale_path(locale)}</loc></url>\n' for locale in catalogs)
    (output / "sitemap.xml").write_text('<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n' + urls + '</urlset>\n')

    app = output / "app/index.html"
    if app.exists():
        html = re.sub(r'<link\b(?=[^>]*\brel=["\'](?:shortcut )?icon["\'])[^>]*>', '', app.read_text())
        app.write_text(html.replace('</head>', icons + '\n</head>'), encoding="utf-8")
    print(f"Built {len(catalogs)} website languages and current application icons")


if __name__ == "__main__":
    build(Path(sys.argv[1] if len(sys.argv) > 1 else "dist"))
