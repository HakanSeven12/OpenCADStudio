"""Run with python3 scripts/test_site.py (Python and Node standard libraries)."""

from datetime import datetime, timezone
from html.parser import HTMLParser
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import tempfile
from urllib.parse import urlsplit
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


class Page(HTMLParser):
    def __init__(self, html):
        super().__init__()
        self.tags = []
        self.ids = set()
        self.feed(html)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        self.tags.append((tag, attrs))
        if 'id' in attrs:
            assert attrs['id'] not in self.ids, attrs['id']
            self.ids.add(attrs['id'])


with tempfile.TemporaryDirectory() as directory:
    output = Path(directory)
    (output / 'app').mkdir()
    (output / 'app/index.html').write_text('<html><head><link rel="icon" href="/old.svg" integrity="old" /></head><body>App</body></html>')
    subprocess.run(['sh', 'scripts/assemble-site.sh', str(output)], cwd=ROOT, check=True)
    catalogs = {p.stem: json.loads(p.read_text()) for p in (ROOT / 'site/locales').glob('*.json')}
    supported = set(re.findall(r'#\[serde\(rename = "([a-z]{2}-[A-Z]{2})"\)\]', (ROOT / 'src/i18n.rs').read_text()))
    assert catalogs.keys() == supported
    spec = importlib.util.spec_from_file_location('charts', ROOT / 'scripts/generate-star-history.py')
    charts = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(charts)
    date = datetime(2026, 1, 1, tzinfo=timezone.utc)
    for locale, messages in catalogs.items():
        for chart in ('stars', 'downloads'):
            svg = charts.render_svg('HakanSeven12/OpenCADStudio', [date], [('v1', date, 10)], 'dark', chart, messages)
            tree = ET.fromstring(svg)
            assert tree.find('{http://www.w3.org/2000/svg}title').text == messages['stars' if chart == 'stars' else 'downloads']
            (output / locale / f'{chart[:-1]}-history-dark.svg').write_text(svg)
        html = (output / locale / 'index.html').read_text()
        assert '{{' not in html and '{count}' not in html, locale
        page = Page(html)
        assert ('html', {'lang': locale, 'dir': 'rtl' if locale == 'ar-SA' else 'ltr'}) in page.tags
        links = [attrs for tag, attrs in page.tags if tag == 'link']
        assert {link['hreflang'] for link in links if link.get('rel') == 'alternate'} == supported | {'x-default'}
        assert next(link['href'] for link in links if link.get('rel') == 'canonical') == f'https://www.opencadstudio.com/{locale}/'
        choices = [attrs for tag, attrs in page.tags if tag == 'a' and 'hreflang' in attrs]
        assert {choice['hreflang'] for choice in choices} == supported
        assert [choice['hreflang'] for choice in choices if choice.get('aria-current') == 'page'] == [locale]
        schema = json.loads(re.search(r'<script type="application/ld\+json">(.*?)</script>', html).group(1))
        assert schema['inLanguage'] == locale and schema['description'] == messages['description']
        for tag, attrs in page.tags:
            if tag == 'img':
                assert 'alt' in attrs and 'width' in attrs and 'height' in attrs
            for attr in ('src', 'href'):
                target = attrs.get(attr, '')
                if target.startswith('#'):
                    assert target[1:] in page.ids, target
                elif target.startswith('/'):
                    path = output / urlsplit(target).path.lstrip('/')
                    assert path.exists(), (locale, target)
        manifest = json.loads((output / locale / 'site.webmanifest').read_text())
        assert manifest['lang'] == locale and manifest['start_url'] == f'/{locale}/'
        for icon in manifest['icons']:
            assert (output / icon['src'].lstrip('/')).exists()
        if locale != 'en-US':
            for key, value in messages.items():
                if len(catalogs['en-US'][key]) > 40:
                    assert value != catalogs['en-US'][key], (locale, key)
    app = (output / 'app/index.html').read_text()
    assert 'old.svg' not in app and 'integrity="old"' not in app and '>App<' in app
    logo = next(attrs['href'] for tag, attrs in Page(app).tags if attrs.get('type') == 'image/svg+xml')
    assert (output / logo.lstrip('/')).read_bytes() == (ROOT / 'assets/logo.svg').read_bytes()
    sitemap = ET.parse(output / 'sitemap.xml')
    assert len(sitemap.getroot()) == len(supported) + 1
    assert (output / 'index.html').read_bytes() == (output / 'en-US/index.html').read_bytes()
    subprocess.run(['node', 'scripts/test_site.cjs', str(output / 'site.js')], cwd=ROOT, check=True)
print(f'All {len(supported)} translations, metadata, local links, charts, manifests and icons passed')
