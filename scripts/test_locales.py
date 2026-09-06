"""Check literal translation lookups. Fluent syntax and rendering are tested in i18n.rs."""

import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
STRING = r'"(?:[^"\\]|\\.)*"'
lookup_text = (ROOT / 'src/locale_catalog.rs').read_text()
lookup = {}
for match in re.finditer(r'(' + STRING + r')\s*=>\s*Some\(\(\s*"([\w-]+)"\s*,\s*"([\w-]+)"\s*,?\s*\)\)', lookup_text):
    source = json.loads(match[1])
    assert source not in lookup, f'Duplicate source lookup: {source}'
    lookup[source] = (match[2], match[3])
assert len(lookup) > 3000

keys = set()
group = None
for line in (ROOT / 'locales/en-US/opencadstudio.ftl').read_text().splitlines():
    if match := re.match(r'^([A-Za-z][\w-]*)\s*=', line):
        group = match[1]
    elif match := re.match(r'^    \.([A-Za-z][\w-]*)\s*=', line):
        keys.add((group, match[1]))
assert not set(lookup.values()) - keys, f'Missing Fluent targets: {set(lookup.values()) - keys}'

missing = []
for path in sorted((ROOT / 'src').rglob('*.rs')):
    text = path.read_text()
    for match in re.finditer(r'\b(?:t|tf)!\(\s*(' + STRING + ')', text, re.S):
        # Rust permits escaped line continuations and literal newlines.
        literal = re.sub(r'\\\n\s*', '', match[1])
        literal = re.sub(r'\\u\{([0-9a-fA-F_]+)\}', lambda m: json.dumps(chr(int(m[1].replace('_', ''), 16)))[1:-1], literal)
        source = json.loads(literal, strict=False)
        if source not in lookup and re.search(r'[A-Za-z]{2}', source):
            line = text[:match.start()].count('\n') + 1
            missing.append(f'{path.relative_to(ROOT)}:{line}: {source!r}')
assert not missing, '\n'.join(missing[:30])
print(f'{len(lookup)} source lookups and literal translation calls passed')
