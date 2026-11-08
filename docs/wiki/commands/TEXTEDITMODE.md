---
summary: Sets whether TEXTEDIT ends after one object or keeps asking for more.
related: TEXTEDIT
---

## Usage
1. Type `TEXTEDITMODE`.
2. Enter the new value: `0` or `Multiple`, `1` or `Single`.

## Notes
- `0` (Multiple, the default): [`TEXTEDIT`](TEXTEDIT-Command) keeps asking for objects until you press Enter.
- `1` (Single): `TEXTEDIT` edits one object and ends.
- Any other value is refused with "Requires 0 OR 1 OR MULTIPLE OR SINGLE."
- The same setting is the "TEXTEDIT edits one object and ends" checkbox in Options. It is kept between sessions, not in the drawing.
